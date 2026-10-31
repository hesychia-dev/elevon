use std::future::Future;
use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use elevon_fs::agent::AgentPath;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, FramedWrite, LengthDelimitedCodec};

pub enum SocketType {
    Proxy,
    Agent,
}

#[derive(Serialize, Deserialize)]
pub enum SocketResponse<R> {
    Log(String),
    Data(R),
    Done,
    Error(String),
}

pub struct Socket {
    pub path: PathBuf,
}

impl Socket {
    pub fn new(socket_type: SocketType, delete: bool) -> Result<Self> {
        let path = match socket_type {
            SocketType::Proxy => AgentPath::ProxySocket,
            SocketType::Agent => AgentPath::AgentSocket,
        }
        .ensure_parent_dir()?;

        if path.exists() && delete {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) => return Err(err.into()),
            }
        }

        Ok(Self { path })
    }

    pub async fn send<M>(&self, msg: M) -> Result<()>
    where
        M: Serialize + Send + 'static,
    {
        let stream = UnixStream::connect(&self.path).await?;
        let mut writer = FramedWrite::new(stream, LengthDelimitedCodec::new());

        let payload_bytes = serde_json::to_vec(&msg)?;
        writer.send(bytes::Bytes::from(payload_bytes)).await?;

        Ok(())
    }

    pub async fn listener<M, R, S, F, Fut>(&self, state: S, handler: F) -> Result<()>
    where
        M: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        S: Clone + Send + Sync + 'static,
        F: Fn(S, M) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<R>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        tracing::info!(path = %self.path.display(), "binding socket");
        let listener = UnixListener::bind(&self.path)
            .with_context(|| format!("failed to bind {}", self.path.display()))?;

        AgentPath::restrict_permissions(&self.path)?;

        let handler = Arc::new(handler);

        loop {
            self.handle_stream(state.clone(), handler.clone(), listener.accept().await)
                .await?;
        }
    }

    async fn handle_stream<M, R, S, F, Fut>(
        &self,
        state: S,
        handler: Arc<F>,
        listener: std::io::Result<(UnixStream, SocketAddr)>,
    ) -> Result<()>
    where
        M: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        S: Clone + Send + Sync + 'static,
        F: Fn(S, M) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<R>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        match listener {
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    let (writer, mut reader) =
                        Framed::new(stream, LengthDelimitedCodec::new()).split();

                    while let Some(result) = reader.next().await {
                        match result {
                            Ok(frame) => {
                                let message: M = match serde_json::from_slice(&frame) {
                                    Ok(m) => m,
                                    Err(err) => {
                                        tracing::error!(%err, "invalid json");
                                        continue; // Keep listening for next frames instead of breaking the loop
                                    }
                                };

                                if let Err(error) =
                                    Self::handle_frame(state, message, handler, writer).await
                                {
                                    tracing::error!(%error, "failed to handle frame");
                                }

                                break;
                            }
                            Err(err) => {
                                tracing::error!(error = %err, "Failed to read framed data from client");
                                break;
                            }
                        }
                    }
                });
            }
            Err(err) => {
                tracing::error!(error = %err, "Fatal error accepting socket connection");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        Ok(())
    }

    async fn handle_frame<M, R, S, F, Fut>(
        state: S,
        message: M,
        handler: Arc<F>,
        mut writer: SplitSink<Framed<UnixStream, LengthDelimitedCodec>, bytes::Bytes>,
    ) -> Result<()>
    where
        M: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        S: Clone + Send + Sync + 'static,
        F: Fn(S, M) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<R>, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + 'static,
    {
        match handler(state, message).await {
            Ok(Some(res)) => {
                let payload = serde_json::to_vec(&res)?;
                if let Err(err) = writer.send(payload.into()).await {
                    tracing::error!(%err, "failed to send response");
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(%err, "Handler execution failed")
            }
        }

        Ok(())
    }
}

pub struct UnixClient {
    pub client: reqwest::Client,
}

impl UnixClient {
    pub fn new() -> Result<Self> {
        let socket = Socket::new(SocketType::Agent, false)?;
        let client = reqwest::Client::builder()
            .unix_socket(socket.path)
            .build()?;

        Ok(Self { client })
    }

    pub fn resolve_url(&self, url: &str) -> String {
        let base_url = self.base_url();
        format!("{}{}", base_url, url)
    }

    fn base_url(&self) -> String {
        "http://localhost".to_string()
    }
}
