mod support;

use serde_json::json;
use support::definition;
use tokio::net::TcpListener;
use v1per_checks::{CheckContext, Status, run};

#[tokio::test]
async fn open_port_passes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let check =
        definition(json!({ "type": "port", "timeout": 5, "hostname": "127.0.0.1", "port": port }));
    let result = run(&check, &CheckContext::new()).await;

    assert_eq!(result.status, Status::Up);
    assert_eq!(result.message, format!("{} ms", result.latency_ms.unwrap()));
}

#[tokio::test]
async fn closed_port_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let check =
        definition(json!({ "type": "port", "timeout": 5, "hostname": "127.0.0.1", "port": port }));
    let result = run(&check, &CheckContext::new()).await;

    assert_eq!(result.status, Status::Down);
    assert_eq!(result.message, "Connection failed");
}
