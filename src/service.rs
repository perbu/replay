use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Empty;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tokio::sync::{mpsc, Semaphore};

use crate::config::MirrorConfig;
use crate::counters::Counters;
use crate::request::MirrorRequest;
use crate::ReplayError;

type HttpClient = Client<HttpConnector, Empty<Bytes>>;

pub struct MirrorHandle {
    task: tokio::task::JoinHandle<()>,
    counters: Counters,
}

impl MirrorHandle {
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    pub async fn shutdown(self) {
        self.task.await.ok();
    }
}

pub fn start(config: MirrorConfig) -> Result<(MirrorHandle, mpsc::Sender<MirrorRequest>), ReplayError> {
    config.validate()?;
    let (tx, rx) = mpsc::channel(config.channel_buffer_size);
    let counters = Counters::default();
    let task = tokio::spawn(worker_loop(rx, config, counters.clone()));
    let handle = MirrorHandle { task, counters };
    Ok((handle, tx))
}

fn sample(rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    rand::random::<f64>() < rate
}

fn is_bodiless(method: &http::Method) -> bool {
    method == http::Method::GET || method == http::Method::HEAD
}

fn build_client() -> HttpClient {
    Client::builder(TokioExecutor::new()).build_http()
}

async fn worker_loop(
    mut rx: mpsc::Receiver<MirrorRequest>,
    config: MirrorConfig,
    counters: Counters,
) {
    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_requests));
    let client = Arc::new(build_client());
    let timeout = config.request_timeout;

    while let Some(request) = rx.recv().await {
        if !is_bodiless(&request.method) {
            counters.increment_dropped();
            continue;
        }

        if !sample(config.sample_rate) {
            counters.increment_dropped();
            continue;
        }

        if request.targets.is_empty() {
            counters.increment_dropped();
            continue;
        }

        let target_count = request.targets.len();

        // All-or-nothing capacity check. Safe from races because this is the
        // only task that acquires permits, and there is no .await between this
        // check and the individual acquires below.
        if semaphore.available_permits() < target_count {
            counters.increment_dropped();
            continue;
        }

        let request = Arc::new(request);
        for target in request.targets.iter() {
            let permit = semaphore.clone().try_acquire_owned().expect(
                "permit must be available: checked above with no intervening await",
            );
            let counters = counters.clone();
            let client = client.clone();
            let request = request.clone();
            let target = *target;
            tokio::spawn(async move {
                let _permit = permit;
                match tokio::time::timeout(timeout, send_mirror(&client, &request, target)).await {
                    Ok(Ok(())) => counters.increment_mirrored(),
                    _ => counters.increment_failed(),
                }
            });
        }
    }
}

async fn send_mirror(
    client: &HttpClient,
    request: &MirrorRequest,
    target: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let uri: http::Uri = format!("http://{}{}", target, request.path).parse()?;

    let mut builder = http::Request::builder()
        .method(request.method.clone())
        .uri(uri);

    for (name, value) in request.headers.iter() {
        builder = builder.header(name, value);
    }

    let req = builder.body(Empty::<Bytes>::new())?;
    let _response = client.request(req).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_always_at_one() {
        for _ in 0..1000 {
            assert!(sample(1.0));
        }
    }

    #[test]
    fn sample_never_at_zero() {
        for _ in 0..1000 {
            assert!(!sample(0.0));
        }
    }

    #[test]
    fn bodiless_methods() {
        assert!(is_bodiless(&http::Method::GET));
        assert!(is_bodiless(&http::Method::HEAD));
        assert!(!is_bodiless(&http::Method::POST));
        assert!(!is_bodiless(&http::Method::PUT));
        assert!(!is_bodiless(&http::Method::DELETE));
    }
}
