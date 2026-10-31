mod socket;
mod state;
mod tls;
pub mod types;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use elevon_http::{check_port, runtime::run_async};
use futures_util::{StreamExt, stream};
use pingora::{
    Error, ErrorType, Result,
    http::ResponseHeader,
    listeners::tls::TlsSettings,
    protocols::l4::socket::SocketAddr,
    proxy::{ProxyHttp, Session, http_proxy_service},
    server::{
        RunArgs, Server, ShutdownWatch,
        configuration::{Opt, ServerConf},
    },
    services::background::{BackgroundService, background_service},
    upstreams::peer::HttpPeer,
};

use crate::{
    env::ElevonEnv,
    proxy::{
        socket::SocketControl,
        state::{ProxyState, not_found_response},
    },
};

pub struct RequestCtx {
    pub deployment_id: Option<String>,
}

pub struct Proxy {
    pub state: Arc<ProxyState>,
}

#[async_trait]
impl ProxyHttp for Proxy {
    type CTX = RequestCtx;
    fn new_ctx(&self) -> Self::CTX {
        RequestCtx {
            deployment_id: None,
        }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let host = session
            .get_header("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");

        if host == self.state.agent.domain {
            let agent_addr = format!("127.0.0.1:{}", self.state.agent.port)
                .parse()
                .expect("agent route must have a valid address");

            return Ok(Box::new(HttpPeer::new(
                SocketAddr::Inet(agent_addr),
                false,
                String::new(),
            )));
        }

        let lbs = self.state.lbs.load();
        let Some(lb) = lbs.get(host) else {
            return Error::e_explain(ErrorType::HTTPStatus(404), "no route for host");
        };

        let upstream = lb
            .select(b"", 256)
            .ok_or_else(|| Error::explain(ErrorType::HTTPStatus(502), "no healthy upstream"))?;

        let selected_port = match upstream.addr {
            SocketAddr::Inet(addr) => Some(addr.port()),
            _ => None,
        };

        if let Some(port) = selected_port {
            let selected = self.state.get_backend_by_port(port);
            if let Some(backend) = selected {
                ctx.deployment_id = Some((*backend.deployment_id).to_string());
            }
        }

        Ok(Box::new(HttpPeer::new(upstream, false, String::new())))
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        if let Ok(file_found) = self.state.handle_static_app_request(session).await {
            if !file_found {
                let _ = not_found_response(session).await;
            }
            return Ok(true);
        }

        if let Some(deployment_id) = ctx.deployment_id.clone() {
            self.state.increase_inflight_count(&deployment_id);
        }

        Ok(false)
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        _upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(deployment_id) = ctx.deployment_id.clone() {
            self.state.decrease_inflight_count(&deployment_id);
        }

        Ok(())
    }
}

pub struct LbHealthCheck {
    pub state: Arc<ProxyState>,
}

#[async_trait]
impl BackgroundService for LbHealthCheck {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                    _ = tick.tick() => {
                        let lbs: Vec<_> = self.state.lbs.load_full().values().cloned().collect();
                        stream::iter(lbs)
                            .for_each_concurrent(32, |lb| async move {
                                lb.backends().run_health_check(true).await;
                            })
                            .await;
                    }
            }
        }
    }
}

pub struct DrainJanitor {
    pub state: Arc<ProxyState>,
}

#[async_trait]
impl BackgroundService for DrainJanitor {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tick.tick() => {
                    let grace = Duration::from_secs(30);
                    for (deployment_id, container_id) in self.state.ready_to_terminate(grace) {
                        // A scenario where this container is a worker, that is working on a task
                        // that takes 60s to finish, `stop_container` will forcefully kill the container,
                        // since Docker first sends SIGTERM, wait for ~10 seconds and then if the container,
                        // doesn't exit automatically Docker sends SIGKILL and forcefully kills the container.
                        // Meaning the worker might not have finished the task it was working on.
                        //
                        // A solution to this would be having a configurable timeout option in `apps.runtime` settings.

                        if let Some(container_id) = container_id {
                            if let Err(err) = self.state.docker
                                .stop_container(&container_id, None)
                                .await
                            {
                                tracing::warn!(%container_id, %err, "failed to stop container");
                                // TODO: If it fails just delete from the db
                                continue;
                            }

                            let _ = self.state.docker
                                .remove_container(&container_id, None)
                                .await;
                        }

                        self.state.remove_backend(&deployment_id);
                        tracing::info!(%deployment_id, "drained deployment terminated");
                        let routes = self.state.routes.load();
                        tracing::info!("current routes: {:?}", routes);
                    }
                }
            }
        }
    }
}

pub fn run_proxy(env: &ElevonEnv, upgrade: bool) -> anyhow::Result<()> {
    // In development, run HTTP on port 6188.
    // If ports are defined in the config file, they are used.
    // Otherwise, runs HTTP on 80 and HTTPS on 443.
    let (http_port, https_port): (String, Option<String>) = if cfg!(debug_assertions) {
        ("6188".to_string(), None)
    } else {
        let (http_port, https_port) = (env.proxy_http_port, env.proxy_https_port);
        let checked_http = check_port(http_port);

        let Some(http_port) = checked_http else {
            tracing::error!("HTTP port is already taken: {}", http_port);
            std::process::exit(1);
        };

        if let Some(https) = https_port {
            let checked_https = check_port(https);
            if checked_https.is_none() {
                tracing::error!("HTTPS port is already taken: {}", http_port);
                std::process::exit(1);
            };
        }

        (http_port.to_string(), https_port.map(|p| p.to_string()))
    };

    let proxy_state = ProxyState::new(env).context("failed to create proxy state")?;

    let config = ServerConf {
        grace_period_seconds: Some(30),
        graceful_shutdown_timeout_seconds: Some(5),
        upgrade_sock: "/tmp/elevon_agent_upgrade.sock".to_string(),
        ..Default::default()
    };

    let server_options = Some(Opt {
        upgrade,
        ..Default::default()
    });

    let mut server = Server::new_with_opt_and_conf(server_options, config);
    server.bootstrap();

    let mut lb = http_proxy_service(
        &server.configuration,
        Proxy {
            state: proxy_state.clone(),
        },
    );

    lb.add_tcp(&format!("0.0.0.0:{}", http_port));
    if let Some(https_port) = &https_port {
        let tls_settings = TlsSettings::with_callbacks(Box::new(proxy_state.dynamic_cert.clone()))
            .expect("failed to initialize TLS settings");

        lb.add_tls_with_settings(&format!("0.0.0.0:{}", https_port), None, tls_settings);
    }

    let control = background_service(
        "socket control",
        SocketControl {
            state: proxy_state.clone(),
        },
    );

    let lb_health_check = background_service(
        "lb health check",
        LbHealthCheck {
            state: proxy_state.clone(),
        },
    );

    let drain_janitor = background_service(
        "drain janitor",
        DrainJanitor {
            state: proxy_state.clone(),
        },
    );

    let cloned_state = proxy_state.clone();
    std::thread::spawn(move || {
        run_async(async move {
            if let Err(err) = cloned_state.load_conainters().await {
                tracing::error!(%err, "initial container load failed");
                std::process::exit(1);
            }
        })
    });

    tracing::info!(port = %http_port, "HTTP proxy listening on");
    if let Some(https_port) = https_port {
        tracing::info!(port = %https_port, "HTTPS proxy listening on");
    } else {
        tracing::debug!("proxy is not being bound to HTTPS");
    }

    server.add_service(lb);
    server.add_service(control);
    server.add_service(lb_health_check);
    server.add_service(drain_janitor);
    server.run(RunArgs::default());

    Ok(())
}
