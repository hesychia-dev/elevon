mod helpers;

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::{Context, Result, bail};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use elevon_contracts::deploy::{
    AppRoutingConfigPayload, AppRoutingPayload, config::StaticRoutingConfig,
};
use elevon_fs::agent::{AgentPath, get_static_app_file};
use pingora::{
    http::ResponseHeader,
    lb::{Backend, LoadBalancer, health_check::TcpHealthCheck, selection::RoundRobin},
    proxy::{ProxyHttp, Session},
    upstreams::peer::HttpPeer,
};

use crate::{
    api::db::models::DeploymentStatus,
    env::ElevonEnv,
    proxy::{
        routing::helpers::{error_response, extract_domain_and_path, file_response},
        tls::DynamicCert,
        types::DeployAppData,
    },
};

pub struct AgentRouting {
    pub domain: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
struct RouteEntry {
    app_id: String,
    deployment_id: String,
    container_id: Option<String>,
    payload: AppRoutingPayload,
}

impl RouteEntry {
    fn from_deploy_data(value: &DeployAppData, routing: &AppRoutingPayload) -> Self {
        Self {
            app_id: value.app_id.clone(),
            deployment_id: value.deployment_id.clone(),
            container_id: value.container_id.clone(),
            payload: routing.clone(),
        }
    }
}

pub struct ProxyRegistry {
    agent: AgentRouting,
    routes: ArcSwap<HashMap<String, Vec<RouteEntry>>>,
    domain_registry: DomainRegistry,
    pub dynamic_cert: Arc<DynamicCert>,
}

impl ProxyRegistry {
    pub fn new(env: &ElevonEnv) -> Self {
        let agent_routing = AgentRouting {
            domain: env.agent_domain.clone(),
            port: env.agent_port,
        };

        let dynamic_cert = DynamicCert::new();
        dynamic_cert.setup_agent_certs(&env.agent_domain).unwrap();

        Self {
            agent: agent_routing,
            routes: ArcSwap::from_pointee(HashMap::new()),
            domain_registry: DomainRegistry::new(),
            dynamic_cert: Arc::new(dynamic_cert),
        }
    }

    pub fn upsert(&self, route: DeployAppData) -> Result<()> {
        let routing = self.check_deployment(&route)?;

        self.routes.rcu(|current| {
            let mut next = current.as_ref().clone();
            let backends = next.entry(routing.domain.clone()).or_default();

            self.dynamic_cert
                .add_cert(&route.app_id, &route.deployment_id, routing.domain.clone())
                .unwrap_or_else(|err| {
                    tracing::error!(
                        domain = %routing.domain,
                        status = ?route.status,
                        error = %err,
                        "failed to save/update certificates"
                    )
                });

            let new_route_entry = RouteEntry::from_deploy_data(&route, routing);

            if let Some(existing) = backends
                .iter_mut()
                .find(|a| a.deployment_id == route.deployment_id)
            {
                *existing = new_route_entry;
            } else {
                backends.push(new_route_entry);
            }

            next
        });

        let routes = self.routes.load();
        let entries = routes
            .get(&routing.domain)
            .map(|table| table.clone())
            .unwrap_or_default();

        self.domain_registry.rebuild(&routing.domain, &entries)?;

        Ok(())
    }

    fn check_deployment<'a>(&self, route: &'a DeployAppData) -> Result<&'a AppRoutingPayload> {
        let Some(routing) = route.routing.as_ref() else {
            bail!("Refusing deploying application without routing config");
        };

        if routing.domain == self.agent.domain {
            bail!(
                "Refusing application route on reserved agent domain '{}'",
                routing.domain
            );
        }

        if route.status != DeploymentStatus::Active {
            tracing::debug!(
                domain = %routing.domain,
                status = ?route.status,
                "skipping inactive application route"
            );
            bail!("Refusing inactive application route");
        }

        Ok(routing)
    }

    fn agent_http_peer(&self) -> Box<HttpPeer> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.agent.port));
        Box::new(HttpPeer::new(addr, false, String::new()))
    }
}

trait RouteBuilder: Sized {
    type BuildProps;

    fn build(props: Self::BuildProps) -> Result<Self>;
}

struct ServerRoute(LoadBalancer<RoundRobin>);
struct StaticRoute(StaticRoutingConfig);

impl RouteBuilder for ServerRoute {
    type BuildProps = Vec<SocketAddr>;

    fn build(props: Self::BuildProps) -> Result<Self> {
        let mut lb = LoadBalancer::<RoundRobin>::try_from_iter(props)
            .context("application routes must have valid addresses")?;

        lb.set_health_check(TcpHealthCheck::new());
        lb.parallel_health_check = true;

        Ok(Self(lb))
    }
}

impl RouteBuilder for StaticRoute {
    type BuildProps = StaticRoutingConfig;

    fn build(props: Self::BuildProps) -> Result<Self> {
        Ok(Self(props))
    }
}

impl ServerRoute {
    fn handle_request(&self) -> pingora::Result<(Backend, u16)> {
        let backend = self
            .0
            .select(b"", 256)
            .ok_or_else(|| error_response("no healthy upstream", 502))?;

        let backend_port = backend
            .addr
            .as_inet()
            .map(|inet| inet.port())
            .ok_or_else(|| error_response("could not get backend port", 502))?;

        Ok((backend, backend_port))
    }
}

impl StaticRoute {
    async fn handle_request(
        &self,
        route_props: &RouteProps,
        session: &mut Session,
    ) -> pingora::Result<()> {
        let path = session.req_header().uri.path();
        let remainded_path = path
            .strip_prefix(route_props.path_prefix.as_deref().unwrap_or_default())
            .unwrap_or_default();

        let static_dir = AgentPath::AppStaticDir(
            route_props.app_id.clone(),
            route_props.deployment_id.clone(),
        );

        let file_path = get_static_app_file(
            &static_dir,
            remainded_path,
            &self.0.directory,
            &self.0.index,
        )
        .ok_or_else(|| error_response("could not find file", 404))?;

        file_response(session, file_path).await
    }
}

trait RouteTargetBuild: Sized {
    fn build_target(self) -> Result<RouteTarget>;
}

impl RouteTargetBuild for AppRoutingConfigPayload {
    fn build_target(self) -> Result<RouteTarget> {
        match self {
            AppRoutingConfigPayload::Server(port) => {
                let new_addr = SocketAddr::from(([127, 0, 0, 1], port));
                let target = ServerRoute::build(vec![new_addr])?;
                Ok(RouteTarget::Server(target))
            }
            AppRoutingConfigPayload::Static(config) => {
                let target = StaticRoute::build(config)?;
                Ok(RouteTarget::Static(target))
            }
        }
    }
}

enum RouteTarget {
    Server(ServerRoute),
    Static(StaticRoute),
}

#[derive(Debug, Clone)]
struct RouteProps {
    app_id: String,
    deployment_id: String,
    path_prefix: Option<String>,
}

#[derive(Clone)]
struct PathRoute {
    props: RouteProps,
    target: Arc<RouteTarget>,
}

#[derive(Clone, Default)]
struct DomainRouteTable(Vec<PathRoute>);

impl DomainRouteTable {
    fn match_path(&self, request_path: &str) -> Option<&PathRoute> {
        self.0.iter().find(|path_balancer| {
            let path_prefix = path_balancer
                .props
                .path_prefix
                .as_deref()
                .unwrap_or_default();
            if let Some(remainder_path) = request_path.strip_prefix(path_prefix) {
                return remainder_path.starts_with("/") || remainder_path.is_empty();
            }
            false
        })
    }

    fn rebuild(entries: &[RouteEntry]) -> Result<Self> {
        let mut new_domain_table: Vec<PathRoute> = Vec::new();

        for entry in entries {
            let route_target = entry.payload.config.clone().build_target()?;

            let path_route = PathRoute {
                props: RouteProps {
                    app_id: entry.app_id.clone(),
                    deployment_id: entry.deployment_id.clone(),
                    path_prefix: entry.payload.path_prefix.clone(),
                },
                target: Arc::new(route_target),
            };

            new_domain_table.push(path_route)
        }

        new_domain_table.sort_by(|a, b| {
            let b_len = b.props.path_prefix.as_deref().unwrap_or_default().len();
            let a_len = a.props.path_prefix.as_deref().unwrap_or_default().len();
            b_len.cmp(&a_len)
        });

        Ok(Self(new_domain_table))
    }
}

struct DomainRegistry {
    tables: ArcSwap<HashMap<String, Arc<DomainRouteTable>>>,
}

impl DomainRegistry {
    fn new() -> Self {
        Self {
            tables: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    fn rebuild(&self, domain: &str, entries: &[RouteEntry]) -> Result<()> {
        let new_table = Arc::new(DomainRouteTable::rebuild(entries)?);

        self.tables.rcu(|current| {
            let mut next = (**current).clone();
            next.insert(domain.to_owned(), new_table.clone());
            Arc::new(next)
        });

        Ok(())
    }

    fn find_path_route(&self, domain: &str, request_path: &str) -> Option<PathRoute> {
        let tables = self.tables.load();
        let table = tables.get(domain)?;

        let path_route = table.clone().match_path(request_path)?.clone();

        Some(path_route)
    }
}

pub struct ProxyService(pub Arc<ProxyRegistry>);

#[derive(Debug, Default)]
pub struct RequestCtx {
    pub deployment_id: Option<String>,
}

#[async_trait]
impl ProxyHttp for ProxyService {
    type CTX = RequestCtx;

    fn new_ctx(&self) -> Self::CTX {
        RequestCtx::default()
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        let (domain, path) = extract_domain_and_path(session)?;

        if domain == self.0.agent.domain {
            return Ok(self.0.agent_http_peer());
        }

        let path_route = self
            .0
            .domain_registry
            .find_path_route(&domain, &path)
            .ok_or_else(|| error_response("could not find app for this route", 404))?;

        if let RouteTarget::Server(server_route) = path_route.target.as_ref() {
            let (backend, _port) = server_route.handle_request()?;

            // TODO: Handle context update

            return Ok(Box::new(HttpPeer::new(backend, false, String::new())));
        }

        Err(error_response(
            "static route should have been handled already",
            502,
        ))
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<bool, Box<pingora::Error>> {
        let (domain, path) = extract_domain_and_path(session)?;

        let Some(path_route) = self.0.domain_registry.find_path_route(&domain, &path) else {
            session.shutdown().await;
            return Ok(true);
        };

        if let RouteTarget::Static(static_route) = path_route.target.as_ref() {
            static_route
                .handle_request(&path_route.props, session)
                .await?;
            return Ok(true);
        }

        // TODO: Handle inflight request count

        Ok(false)
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        _upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        // TODO: Handle inflight request count

        Ok(())
    }
}
