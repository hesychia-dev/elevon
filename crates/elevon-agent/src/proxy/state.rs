use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use axum::http::HeaderValue;
use bytes::Bytes;
use dashmap::DashMap;
use elevon_fs::agent::{AgentPath, get_static_app_file};
use futures_util::StreamExt;
use pingora::{
    http::ResponseHeader,
    lb::{LoadBalancer, health_check::TcpHealthCheck, selection::RoundRobin},
    proxy::Session,
};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::{
    api::db::models::DeploymentStatus,
    env::ElevonEnv,
    proxy::{
        tls::DynamicCert,
        types::{BackendRuntime, DeployAppData, DrainAppData, RouteBackendRuntime},
    },
    socket::{Socket, SocketType, UnixClient},
};

pub struct AgentState {
    pub domain: String,
    pub port: u16,
}

pub struct ProxyState {
    pub agent: AgentState,
    pub docker: Arc<bollard::Docker>,
    pub routes: ArcSwap<HashMap<String, Vec<DeployAppData>>>,
    pub lbs: ArcSwap<HashMap<String, Arc<LoadBalancer<RoundRobin>>>>,
    pub runtime: DashMap<String, Arc<BackendRuntime>>,
    pub dynamic_cert: DynamicCert,
    pub proxy_socket: Arc<Socket>,
}

impl ProxyState {
    pub fn new(env: &ElevonEnv) -> anyhow::Result<Arc<Self>> {
        let Some(agent_domain) = &env.agent_domain else {
            tracing::error!(
                "agent domain was not provided, run `elevon-agent install` to use the new configuration"
            );
            std::process::exit(1);
        };

        let dynamic_cert = DynamicCert::new();

        dynamic_cert.setup_agent_certs(agent_domain)?;

        let agent_state = AgentState {
            domain: agent_domain.clone(),
            port: env.agent_port,
        };

        let proxy_socket = Socket::new(SocketType::Proxy, true)?;

        Ok(Arc::new(ProxyState {
            agent: agent_state,
            docker: Arc::new(bollard::Docker::connect_with_defaults().unwrap()),
            routes: ArcSwap::from_pointee(HashMap::new()),
            lbs: ArcSwap::from_pointee(HashMap::new()),
            runtime: DashMap::new(),
            dynamic_cert,
            proxy_socket: Arc::new(proxy_socket),
        }))
    }

    pub async fn load_conainters(&self) -> Result<()> {
        let unix_client = UnixClient::new()?;

        for _ in 0..20 {
            let response = unix_client
                .client
                .get(unix_client.resolve_url("/deploy/local/containers"))
                .send()
                .await;

            match response {
                Ok(res) => {
                    let containers: Vec<DeployAppData> = res.json().await?;

                    if containers.is_empty() {
                        return Ok(());
                    }

                    tracing::info!(
                        "Found {} running containers, upserting them...",
                        containers.len()
                    );

                    for container in containers {
                        self.upsert_route(container);
                    }

                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(%err, "proxy containers request error, retrying");
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        anyhow::bail!("failed to load initial containers from API");
    }

    pub fn upsert_route(&self, route: DeployAppData) {
        let Some((domain, port)) = route
            .routing
            .as_ref()
            .map(|r| (r.domain.clone(), r.resolved_port))
        else {
            tracing::warn!(deployment = %&route.deployment_id, "refusing application route on non web app");
            return;
        };

        if domain == self.agent.domain {
            tracing::warn!(
                domain = %domain,
                "refusing application route on reserved agent domain"
            );
            return;
        }

        if route.status != DeploymentStatus::Active {
            tracing::debug!(
                domain = %domain,
                status = ?route.status,
                "skipping inactive application route"
            );
            return;
        }

        self.routes.rcu(|current| {
            let mut next = current.as_ref().clone();
            let backends = next.entry(domain.clone()).or_default();

            self.dynamic_cert
                .add_cert(&route.app_id, &route.deployment_id, domain.clone())
                .unwrap_or_else(|err| {
                    tracing::error!(
                        domain = %domain,
                        status = ?route.status,
                        error = %err,
                        "failed to save/update certificates"
                    )
                });

            if let Some(existing) = backends
                .iter_mut()
                .find(|a| a.deployment_id == route.deployment_id)
            {
                *existing = route.clone();
            } else {
                backends.push(route.clone());
            }

            next
        });

        // Getting to this means that its a static app so we don't need to touch LB
        let Some(port) = port else {
            tracing::info!("Routes changed, current state: {:?}", self.routes.load());
            return;
        };

        let routes = self.routes.load();
        let backends = routes.get(&domain).cloned().unwrap_or_default();

        let addrs: Vec<String> = backends
            .iter()
            .filter(|b| b.status == DeploymentStatus::Active)
            .map(|_| format!("127.0.0.1:{}", port))
            .collect();

        let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(addrs).expect("valid backends");
        lb.set_health_check(TcpHealthCheck::new());
        lb.parallel_health_check = true;
        let lb = Arc::new(lb);

        self.lbs.rcu(|current| {
            let mut next = (**current).clone();
            next.insert(domain.clone(), lb.clone());
            next
        });

        tracing::info!("Routes changed, current state: {:?}", self.routes.load());
    }

    pub fn drain_app(&self, app: DrainAppData) {
        let mut backend_runtime = BackendRuntime {
            deployment_id: app.deployment_id.clone(),
            container_id: app.container_id.clone(),
            status: DeploymentStatus::Drained,
            ..Default::default()
        };

        if let Some(domain) = app.domain {
            self.routes.rcu(|current| {
                let mut next = current.as_ref().clone();
                let backends = next.entry(domain.clone()).or_default();

                for backend in backends.iter_mut() {
                    if backend.deployment_id == app.deployment_id.clone() {
                        backend.status = DeploymentStatus::Drained;
                    }
                }

                next
            });

            self.rebuild_load_balancer(domain);
        }

        if let Some(port) = app.port {
            let inflight = self
                .runtime
                .get(&app.deployment_id)
                .map(|r| {
                    r.route
                        .as_ref()
                        .map(|ro| ro.inflight.load(std::sync::atomic::Ordering::Relaxed))
                        .unwrap_or(0)
                })
                .unwrap_or(0);

            let route_backend_runtime = RouteBackendRuntime {
                port,
                inflight: AtomicUsize::new(inflight),
                drain_started_at: Some(Instant::now()),
            };

            backend_runtime.route = Some(route_backend_runtime);
        }

        self.runtime
            .insert(app.deployment_id.clone(), Arc::new(backend_runtime));

        tracing::info!(deployment = %&app.deployment_id, "App marked as draining");
    }

    pub fn get_backend_by_port(&self, port: u16) -> Option<Arc<BackendRuntime>> {
        self.runtime
            .iter()
            .find(|r| r.value().route.as_ref().is_some_and(|ro| ro.port == port))
            .map(|r| r.value().clone())
    }

    pub fn increase_inflight_count(&self, deployment_id: &str) {
        if let Some(route) = self
            .runtime
            .get(deployment_id)
            .as_ref()
            .and_then(|r| r.route.as_ref())
        {
            route.inflight.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn decrease_inflight_count(&self, deployment_id: &str) {
        if let Some(route) = self
            .runtime
            .get(deployment_id)
            .as_ref()
            .and_then(|r| r.route.as_ref())
        {
            route.inflight.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Returns (deployment_id, Some(container_id))
    pub fn ready_to_terminate(&self, grace_period: Duration) -> Vec<(String, Option<String>)> {
        self.runtime
            .iter()
            .filter_map(|r| {
                let deployment_id = r.key();
                let runtime = r.value();

                // If not drained, filter it out entirely
                if runtime.status != DeploymentStatus::Drained {
                    return None;
                }

                let container_opt = match runtime.route.as_ref() {
                    Some(route) => {
                        let inflight = route.inflight.load(std::sync::atomic::Ordering::Relaxed);
                        let past_grace = route
                            .drain_started_at
                            .is_some_and(|t| t.elapsed() >= grace_period);

                        if inflight == 0 || past_grace {
                            runtime.container_id.clone()
                        } else {
                            None
                        }
                    }
                    None => runtime.container_id.clone(),
                };

                Some((deployment_id.to_string(), container_opt))
            })
            .collect()
    }

    pub fn remove_backend(&self, deployment_id: &str) {
        let Some((_, _)) = self.runtime.remove(deployment_id) else {
            return;
        };

        let web_domains: Vec<String> = self
            .routes
            .load()
            .iter()
            .filter(|(_, routes)| {
                routes
                    .iter()
                    .any(|r| r.deployment_id == deployment_id && r.web_app.is_some())
            })
            .map(|(domain, _)| domain.clone())
            .collect();

        self.routes.rcu(|current| {
            let mut next = current.as_ref().clone();

            for backends in next.values_mut() {
                backends.retain(|b| b.deployment_id != deployment_id);
            }

            next.retain(|_, backends| !backends.is_empty());
            next
        });

        for domain in web_domains {
            self.rebuild_load_balancer(domain);
        }
    }

    fn rebuild_load_balancer(&self, domain: String) {
        let routes = self.routes.load();

        let addrs: Vec<String> = routes
            .get(&domain)
            .into_iter()
            .flatten()
            .filter_map(|route| {
                let web_app = route.web_app.clone()?;

                if route.status != DeploymentStatus::Active {
                    return None;
                }

                Some(format!("127.0.0.1:{}", web_app.port))
            })
            .collect();

        if addrs.is_empty() {
            self.lbs.rcu(|current| {
                let mut next = (**current).clone();
                next.remove(&domain);
                next
            });
            return;
        }

        let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(addrs)
            .expect("active application routes must have valid addresses");

        lb.set_health_check(TcpHealthCheck::new());
        lb.parallel_health_check = true;

        let lb = Arc::new(lb);

        self.lbs.rcu(|current| {
            let mut next = (**current).clone();
            next.insert(domain.to_string(), lb.clone());
            next
        });
    }

    /// Returning error means it's not a static app,
    /// returning false means - it's a static app but the file not found
    /// returning true means it's successful
    pub async fn handle_static_app_request(&self, session: &mut Session) -> Result<bool> {
        let host = session
            .req_header()
            .headers
            .get("Host")
            .context("Host not found in headers")?;
        let path = session.req_header().uri.path();

        let domain = extract_domain(host).context("Domain not found")?;

        let guard = self.routes.load();
        let routes = guard.get(domain).context("Route not found")?;

        let (app_id, deployment_id, static_app) =
            find_static_app(routes, path).context("Static app not found")?;

        let static_dir = AgentPath::AppStaticDir(app_id, deployment_id);

        let clean_path = static_app
            .path
            .as_deref()
            .and_then(|p| path.strip_prefix(p))
            .unwrap_or(path);

        let Some(file_path) = get_static_app_file(
            &static_dir,
            clean_path,
            &static_app.directory,
            &static_app.index,
        ) else {
            return Ok(false);
        };

        if file_response(session, file_path).await.is_err() {
            return Ok(false);
        }

        Ok(true)
    }
}

fn find_static_app<'a>(
    routes: &'a [DeployAppData],
    path: &'a str,
) -> Option<(String, String, &'a StaticApp)> {
    routes.iter().find_map(|route| {
        if route.status != DeploymentStatus::Active {
            return None;
        }
        route
            .static_app
            .as_ref()
            .filter(|app| {
                app.path.as_deref().is_none_or(|p| {
                    let rest = path.strip_prefix(p).unwrap_or(path);
                    rest.starts_with('/') || rest.starts_with('?')
                })
            })
            .map(|s| (route.app_id.clone(), route.deployment_id.clone(), s))
    })
}

fn extract_domain(host: &HeaderValue) -> Option<&str> {
    let host_str = host.to_str().ok()?;

    let mut parts = host_str.rsplitn(2, ':');
    let first_part = parts.next()?;

    if let Some(domain_part) = parts.next() {
        // If there was a colon, the second part yielded by rsplitn is the domain
        Some(domain_part)
    } else {
        // If there was no colon, the whole string is the domain
        Some(first_part)
    }
}

pub async fn not_found_response(session: &mut Session) -> Result<()> {
    let mut header = ResponseHeader::build(404, Some(3))?;
    header.insert_header("Content-Type", "text/plain")?;

    session
        .write_response_header(Box::new(header), false)
        .await?;
    session
        .write_response_body(Some(Bytes::from("404 Not Found")), true)
        .await?;

    Ok(())
}

async fn file_response(session: &mut Session, file_path: std::path::PathBuf) -> Result<()> {
    let file = File::open(&file_path).await?;

    let mut header = ResponseHeader::build(200, None)?;
    let mime_guess = mime_guess::from_path(&file_path)
        .first_raw()
        .unwrap_or("application/octet-stream");

    header.insert_header("Content-Type", mime_guess)?;

    session
        .write_response_header(Box::new(header), false)
        .await?;

    let mut file_stream = ReaderStream::new(file);
    while let Some(chunk) = file_stream.next().await {
        let chunk = chunk?;
        session.write_response_body(Some(chunk), false).await?;
    }

    session.write_response_body(None, true).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_find_static_app() {
    //     let placeholder = "test";

    //     let app_1 = DeployAppData {
    //         app_id: placeholder.to_string(),
    //         deployment_id: placeholder.to_string(),
    //         status: DeploymentStatus::Active,
    //         static_app: Some(StaticApp {
    //             domain: placeholder.to_string(),
    //             directory: placeholder.to_string(),
    //             index: placeholder.to_string(),
    //             path: Some("/api/v1".to_string()),
    //         }),
    //         ..Default::default()
    //     };

    //     let app_2 = DeployAppData {
    //         app_id: placeholder.to_string(),
    //         deployment_id: placeholder.to_string(),
    //         status: DeploymentStatus::Active,
    //         static_app: Some(StaticApp {
    //             domain: placeholder.to_string(),
    //             directory: placeholder.to_string(),
    //             index: placeholder.to_string(),
    //             path: Some("/api".to_string()),
    //         }),
    //         ..Default::default()
    //     };

    //     let mut routes: Vec<DeployAppData> = vec![app_1, app_2];
    //     routes.sort_by(|a, b| b.);

    //     let selected_1 = find_static_app(&routes, "/api/v1/test");

    //     println!("selected: {:?}", selected_1);
    // }

    #[test]
    fn test_extract_domain() -> Result<()> {
        assert_eq!(
            extract_domain(&HeaderValue::from_static("test.com:8000")),
            Some("test.com")
        );

        assert_eq!(
            extract_domain(&HeaderValue::from_static("test.com")),
            Some("test.com")
        );

        Ok(())
    }
}
