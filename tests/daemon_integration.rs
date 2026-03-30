// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::test]
#[ignore = "requires full daemon environment and network setup"]
async fn test_daemon_full_flow() {
    // Set a known token so the test can authenticate.
    std::env::set_var("IRONTRACK_TOKEN", "integration_test_token_abc123");

    // 1. Start daemon in background (port 8081 to avoid conflict)
    let port = 8081;
    tokio::spawn(async move {
        irontrack::network::server::run_server(port, Some(15.0), 0, 0, vec![])
            .await
            .unwrap();
    });

    // Give it a moment to start
    sleep(Duration::from_millis(500)).await;

    let client = Client::new();
    let base_url = format!("http://127.0.0.1:{port}");
    let token = "integration_test_token_abc123";

    // 2. POST mission create
    let load_req = json!({
        "min_lat": 51.0,
        "min_lon": 0.0,
        "max_lat": 51.01,
        "max_lon": 0.01,
        "sensor": "phantom4pro",
        "gsd_cm": 5.0,
        "side_lap": 60.0,
        "end_lap": 80.0,
        "altitude_msl": 600.0,
        "mode": "area",
        "terrain": false,
        "datum": "egm2008"
    });

    let resp = client
        .post(format!("{base_url}/api/v1/missions"))
        .bearer_auth(token)
        .json(&load_req)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let summary: Value = resp.json().await.unwrap();
    let line_count = summary["line_count"].as_u64().unwrap();
    assert!(line_count > 0);
    let id = summary["id"].as_u64().unwrap();

    // 3. Connect WebSocket with token
    let ws_url = format!("ws://127.0.0.1:{port}/ws?token={token}");
    let (mut ws_stream, _) = connect_async(ws_url)
        .await
        .expect("Failed to connect to WS");

    // 4. Verify initial frame (SystemStatus)
    let msg = ws_stream.next().await.unwrap().unwrap();
    if let Message::Text(text) = msg {
        let status: Value = serde_json::from_str(&text).unwrap();
        assert!(status.get("line_statuses").is_some());
    }

    // 5. Verify telemetry stream (CurrentState)
    let msg = ws_stream.next().await.unwrap().unwrap();
    if let Message::Text(text) = msg {
        let telemetry: Value = serde_json::from_str(&text).unwrap();
        assert!(telemetry.get("lat").is_some());
    }

    // 6. PUT update line status
    let line_update = json!({
        "completion_pct": 50.0,
        "is_active": true
    });

    let resp = client
        .put(format!("{base_url}/api/v1/missions/{id}/lines/0"))
        .bearer_auth(token)
        .json(&line_update)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());

    // 7. Verify WS receives update
    // We might need to drain some telemetry messages first
    let mut found_update = false;
    for _ in 0..20 {
        let msg = ws_stream.next().await.unwrap().unwrap();
        if let Message::Text(text) = msg {
            let val: Value = serde_json::from_str(&text).unwrap();
            if let Some(statuses) = val.get("line_statuses") {
                if statuses[0]["completion_pct"].as_f64().unwrap() == 50.0 {
                    found_update = true;
                    break;
                }
            }
        }
    }
    assert!(found_update, "WS did not receive line update");
}
