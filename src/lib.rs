mod config;
mod counters;
mod request;
mod service;

pub use config::MirrorConfig;
pub use counters::Counters;
pub use request::MirrorRequest;
pub use service::MirrorHandle;

pub use http::{HeaderMap, Method};

pub fn start(config: MirrorConfig) -> Result<(MirrorHandle, tokio::sync::mpsc::Sender<MirrorRequest>), ReplayError> {
    service::start(config)
}

#[derive(Debug)]
pub enum ReplayError {
    InvalidConfig(String),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::InvalidConfig(msg) => write!(f, "invalid config: {msg}"),
        }
    }
}

impl std::error::Error for ReplayError {}
