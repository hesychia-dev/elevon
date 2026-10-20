use std::{collections::HashMap, sync::Arc};

use arc_swap::ArcSwap;
use axum::Router;
use tower_http::services::ServeDir;

#[derive(Debug)]
pub struct ElevonServe {
    pub domains: ArcSwap<HashMap<String, DirectoryDomainInfo>>,
    router: Router,
}

impl ElevonServe {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            domains: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    pub fn add_static_dir(mut self, domain: &str, dir: &str) -> Self {
        self.router = self.router.nest_service(domain, ServeDir::new(dir));
        self
    }

    pub fn build(self) -> Arc<Self> {
        Arc::new(self)
    }
}

#[derive(Debug, Clone)]
pub struct DirectoryDomainInfo {
    pub target_dir: String,
    pub index_file: String,
}
