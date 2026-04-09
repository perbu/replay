use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use replay::{HeaderMap, Method, MirrorConfig, MirrorRequest};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

/// Spawn a TCP listener that accepts connections, reads the request, sends a
/// minimal 200 response, and increments a counter.
async fn start_test_server() -> (SocketAddr, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let counter = counter_clone.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                counter.fetch_add(1, Ordering::Relaxed);
                let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response).await;
            });
        }
    });

    (addr, counter)
}

fn get_request(path: &str, targets: Vec<SocketAddr>) -> MirrorRequest {
    let mut headers = HeaderMap::new();
    headers.insert("host", "example.com".parse().unwrap());
    MirrorRequest {
        method: Method::GET,
        path: path.to_string(),
        headers,
        targets,
    }
}

/// Wait for a counter to reach the expected value, with a timeout.
async fn wait_for_counter(counter: &AtomicU64, expected: u64) {
    for _ in 0..100 {
        if counter.load(Ordering::Relaxed) >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "counter did not reach {expected} within 1s (got {})",
        counter.load(Ordering::Relaxed)
    );
}

async fn wait_for_counters_stable(counters: &replay::Counters, expected_total: u64) {
    for _ in 0..100 {
        let total = counters.mirrored() + counters.failed() + counters.dropped();
        if total >= expected_total {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn mirror_single_target() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    tx.try_send(get_request("/hello?q=1", vec![addr])).unwrap();

    wait_for_counter(&server_counter, 1).await;
    assert_eq!(server_counter.load(Ordering::Relaxed), 1);
    assert_eq!(handle.counters().mirrored(), 1);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn mirror_fan_out() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    tx.try_send(get_request("/fanout", vec![addr, addr, addr]))
        .unwrap();

    wait_for_counter(&server_counter, 3).await;
    assert_eq!(handle.counters().mirrored(), 3);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn non_get_filtered() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    let mut headers = HeaderMap::new();
    headers.insert("host", "example.com".parse().unwrap());
    tx.try_send(MirrorRequest {
        method: Method::POST,
        path: "/post".into(),
        headers,
        targets: vec![addr],
    })
    .unwrap();

    wait_for_counters_stable(handle.counters(), 1).await;
    assert_eq!(server_counter.load(Ordering::Relaxed), 0);
    assert_eq!(handle.counters().dropped(), 1);
    assert_eq!(handle.counters().mirrored(), 0);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn sampling_at_zero_drops_all() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 0.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    for _ in 0..50 {
        tx.try_send(get_request("/nope", vec![addr])).unwrap();
    }

    wait_for_counters_stable(handle.counters(), 50).await;
    assert_eq!(server_counter.load(Ordering::Relaxed), 0);
    assert_eq!(handle.counters().dropped(), 50);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn capacity_drop() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 1.0,
        max_concurrent_requests: 2,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    // 3 targets but only 2 slots — should be dropped
    tx.try_send(get_request("/too-many", vec![addr, addr, addr]))
        .unwrap();

    wait_for_counters_stable(handle.counters(), 1).await;
    assert_eq!(handle.counters().dropped(), 1);
    assert_eq!(server_counter.load(Ordering::Relaxed), 0);

    // 2 targets, fits in 2 slots — should succeed
    tx.try_send(get_request("/fits", vec![addr, addr])).unwrap();

    wait_for_counter(&server_counter, 2).await;
    assert_eq!(handle.counters().mirrored(), 2);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn connection_failure_counted() {
    // Target a port where nothing is listening
    let bad_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();

    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    tx.try_send(get_request("/fail", vec![bad_addr])).unwrap();

    wait_for_counters_stable(handle.counters(), 1).await;
    assert_eq!(handle.counters().failed(), 1);
    assert_eq!(handle.counters().mirrored(), 0);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn shutdown_completes() {
    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();
    drop(tx);

    tokio::time::timeout(Duration::from_secs(2), handle.shutdown())
        .await
        .expect("shutdown should complete within 2s");
}

#[tokio::test]
async fn head_request_mirrored() {
    let (addr, server_counter) = start_test_server().await;

    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    let mut headers = HeaderMap::new();
    headers.insert("host", "example.com".parse().unwrap());
    tx.try_send(MirrorRequest {
        method: Method::HEAD,
        path: "/head".into(),
        headers,
        targets: vec![addr],
    })
    .unwrap();

    wait_for_counter(&server_counter, 1).await;
    assert_eq!(handle.counters().mirrored(), 1);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn request_timeout() {
    // Bind a listener but never respond — should trigger timeout
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            // Accept but never respond; hold the connection open
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                tokio::time::sleep(Duration::from_secs(60)).await;
            });
        }
    });

    let config = MirrorConfig {
        sample_rate: 1.0,
        request_timeout: Duration::from_millis(100),
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    tx.try_send(get_request("/slow", vec![addr])).unwrap();

    wait_for_counters_stable(handle.counters(), 1).await;
    assert_eq!(handle.counters().failed(), 1);
    assert_eq!(handle.counters().mirrored(), 0);

    drop(tx);
    handle.shutdown().await;
}

#[tokio::test]
async fn empty_targets_dropped() {
    let config = MirrorConfig {
        sample_rate: 1.0,
        ..MirrorConfig::default()
    };
    let (handle, tx) = replay::start(config).unwrap();

    tx.try_send(get_request("/nowhere", vec![])).unwrap();

    wait_for_counters_stable(handle.counters(), 1).await;
    assert_eq!(handle.counters().dropped(), 1);

    drop(tx);
    handle.shutdown().await;
}
