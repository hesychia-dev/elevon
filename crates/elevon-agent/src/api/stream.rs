use std::{
    convert::Infallible,
    fs::{OpenOptions, TryLockError},
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use axum::response::{
    Sse,
    sse::{Event, KeepAlive, KeepAliveStream},
};
use elevon_contracts::deploy::{StreamEvent, StreamLogLevel};
use elevon_fs::agent::AgentPath;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;

type StreamItem = Result<Event, Infallible>;
pub type StreamSender = Sender<StreamItem>;
pub type StreamReceiver = Receiver<StreamItem>;
pub type StreamResponse = Sse<KeepAliveStream<ReceiverStream<StreamItem>>>;

pub fn create_stream_channel() -> (StreamSender, StreamReceiver) {
    mpsc::channel::<StreamItem>(32)
}

pub async fn emit(tx: &StreamSender, event: StreamEvent) {
    let line = serde_json::to_string(&event).unwrap();
    let _ = tx.send(Ok(Event::default().data(line))).await;
}

pub enum LogInput {
    Event(StreamEvent),
    Message(String),
}

impl From<StreamEvent> for LogInput {
    fn from(event: StreamEvent) -> Self {
        LogInput::Event(event)
    }
}

impl From<String> for LogInput {
    fn from(s: String) -> Self {
        LogInput::Message(s)
    }
}

impl From<LogInput> for StreamEvent {
    fn from(value: LogInput) -> Self {
        match value {
            LogInput::Event(event) => event,
            LogInput::Message(msg) => StreamEvent::log(msg),
        }
    }
}

#[async_trait]
pub trait ActionLogger: Send + Sync {
    async fn log(&self, input: LogInput) -> Result<()>;
}

#[async_trait]
impl ActionLogger for StreamSender {
    async fn log(&self, input: LogInput) -> Result<()> {
        emit(self, input.into()).await;
        Ok(())
    }
}

pub async fn emit_error(tx: &StreamSender, message: String) {
    emit(tx, StreamEvent::Error { message }).await;
}

pub fn stream_response(rx: StreamReceiver) -> StreamResponse {
    let stream = ReceiverStream::new(rx);
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep alive"),
    )
}

pub fn spawn_streaming_task<F, Fut>(op: F) -> StreamResponse
where
    F: FnOnce(StreamSender) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let (tx, rx) = create_stream_channel();
    let tx_for_task = tx.clone();

    tokio::spawn(async move {
        if let Err(err) = op(tx_for_task.clone()).await {
            emit(
                &tx_for_task,
                StreamEvent::Error {
                    message: err.to_string(),
                },
            )
            .await;
        }

        emit(&tx_for_task, StreamEvent::Done).await;
    });

    stream_response(rx)
}

pub fn lock_action() -> anyhow::Result<std::fs::File> {
    let lock_path = AgentPath::DeployLock.resolve();
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(lock_path)?;

    match lock_file.try_lock() {
        Ok(()) => Ok(lock_file),
        Err(TryLockError::WouldBlock) => {
            anyhow::bail!("another deployment is already running")
        }
        Err(TryLockError::Error(err)) => Err(err.into()),
    }
}

#[async_trait]
pub trait OptionLoggerExt {
    async fn log_event(&self, input: LogInput, level: StreamLogLevel) -> Result<()>;
}

#[async_trait]
impl OptionLoggerExt for Option<&(dyn ActionLogger + Send + Sync)> {
    async fn log_event(&self, input: LogInput, level: StreamLogLevel) -> Result<()> {
        if let Some(logger) = self {
            logger.log(input).await?;
        } else {
            let msg = match input {
                LogInput::Event(event) => match event {
                    StreamEvent::Log { level: _, message } => Some(message),
                    StreamEvent::Error { message } => Some(message),
                    _ => None,
                },
                LogInput::Message(msg) => Some(msg),
            };

            if let Some(msg) = msg {
                match level {
                    StreamLogLevel::Info => tracing::info!(%msg),
                    StreamLogLevel::Warn => tracing::warn!(%msg),
                    StreamLogLevel::Error => tracing::error!(%msg),
                }
            }
        }
        Ok(())
    }
}
