use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use elevon_contracts::deploy::TlsType;
use elevon_fs::agent::AppEnvOptions;
use elevon_fs::agent::tls::get_tls_file;
use pingora::protocols::tls::TlsRef;
use pingora::tls::pkey::{PKey, Private};
use pingora::tls::ssl::NameType;
use pingora::tls::x509::X509;

type Certs = ArcSwap<HashMap<String, (X509, PKey<Private>)>>;

pub struct DynamicCert(Certs);

impl DynamicCert {
    pub fn new() -> Self {
        Self(ArcSwap::new(Arc::new(HashMap::new())))
    }

    pub fn setup_agent_certs(&self, domain: &str) -> Result<()> {
        let options = AppEnvOptions::agent();

        let stored_cert = get_tls_file(TlsType::Cert, options.clone())?;
        let stored_key = get_tls_file(TlsType::Key, options)?;

        self.add_cert_from_paths(
            domain.to_string(),
            stored_cert.to_str().context("invalid certificate path")?,
            stored_key.to_str().context("invalid key path")?,
        )?;

        Ok(())
    }

    pub fn add_cert(&self, app_id: &str, deployment_id: &str, domain: String) -> Result<()> {
        let (stored_cert, stored_key) = self.get_stored_key_paths(app_id, deployment_id)?;

        self.add_cert_from_paths(
            domain,
            stored_cert.to_str().context("invalid certificate path")?,
            stored_key.to_str().context("invalid key path")?,
        )
    }

    fn get_stored_key_paths(
        &self,
        app_id: &str,
        deployment_id: &str,
    ) -> Result<(PathBuf, PathBuf)> {
        let options = AppEnvOptions::app(app_id, deployment_id);

        let stored_cert = get_tls_file(TlsType::Cert, options.clone())?;
        let stored_key = get_tls_file(TlsType::Key, options)?;

        Ok((stored_cert, stored_key))
    }

    fn add_cert_from_paths(&self, domain: String, cert_path: &str, key_path: &str) -> Result<()> {
        let (cert_bytes, key_bytes) = match (std::fs::read(cert_path), std::fs::read(key_path)) {
            (Ok(cert), Ok(key)) => (cert, key),
            _ => return Ok(()),
        };

        let cert = X509::from_pem(&cert_bytes)?;
        let key = PKey::private_key_from_pem(&key_bytes)?;

        self.0.rcu(|current| {
            let mut next = current.as_ref().clone();
            next.insert(domain.clone(), (cert.clone(), key.clone()));
            next
        });

        Ok(())
    }

    fn find_certs_with_hostname(&self, hostname: &str) -> Option<(Arc<X509>, Arc<PKey<Private>>)> {
        self.0
            .load()
            .get(hostname)
            .map(|(c, k)| (Arc::new(c.clone()), Arc::new(k.clone())))
    }
}

pub struct SharedTlsCert(pub Arc<DynamicCert>);

#[async_trait]
impl pingora::listeners::TlsAccept for SharedTlsCert {
    async fn certificate_callback(&self, ssl: &mut TlsRef) {
        use pingora::tls::ext;

        if self.0.0.load().is_empty() {
            tracing::warn!("No certificates are configured.");
            return;
        }

        if let Some(server_name) = ssl.servername(NameType::HOST_NAME)
            && let Some((cert, key)) = self.0.find_certs_with_hostname(server_name)
        {
            ext::ssl_use_certificate(ssl, &cert).unwrap();
            ext::ssl_use_private_key(ssl, &key).unwrap();
            return;
        }
    }
}
