// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later

use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use irontrack::network::server::{start_test_server, AppState, FmsState};
use irontrack::network::telemetry::TriggerEvent;

/// Create an `AppState` with a known token for test use.
fn make_test_state(token: &str) -> AppState {
    let state = Arc::new(RwLock::new(FmsState::new().unwrap()));
    AppState {
        fms: state,
        token: Arc::new(token.to_string()),
    }
}

/// Helper: connect to the WS endpoint with the given token.
async fn ws_connect(
    addr: std::net::SocketAddr,
    token: &str,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let url = format!("ws://127.0.0.1:{}/ws?token={}", addr.port(), token);
    let (stream, _) = connect_async(url).await.expect("WS connect failed");
    stream.split()
}

/// Read the next Text message from the stream, with a timeout.
async fn next_text(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout: Duration,
) -> Option<Value> {
    let deadline = tokio::time::timeout(timeout, async {
        loop {
            match read.next().await {
                Some(Ok(Message::Text(text))) => {
                    return Some(serde_json::from_str::<Value>(&text).unwrap());
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Close(_))) | None => return None,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return None,
            }
        }
    });
    deadline.await.unwrap_or(None)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Connect with valid token — upgrade succeeds.
/// Connect with invalid token — rejected.
#[tokio::test]
async fn ws_connect_valid_token() {
    let token = "test_token_ws_auth";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    // Valid token should connect
    let url_ok = format!("ws://127.0.0.1:{}/ws?token={}", addr.port(), token);
    let result = connect_async(&url_ok).await;
    assert!(result.is_ok(), "valid token should connect");

    // Invalid token should be rejected
    let url_bad = format!("ws://127.0.0.1:{}/ws?token=wrong", addr.port());
    let result = connect_async(&url_bad).await;
    assert!(result.is_err(), "invalid token should be rejected");

    // Missing token should be rejected
    let url_none = format!("ws://127.0.0.1:{}/ws", addr.port());
    let result = connect_async(&url_none).await;
    assert!(result.is_err(), "missing token should be rejected");
}

/// First frame must be an INIT envelope with full SystemStatus fields.
#[tokio::test]
async fn ws_init_frame_contains_full_state() {
    let token = "test_token_init";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (_write, mut read) = ws_connect(addr, token).await;

    let init = next_text(&mut read, Duration::from_secs(5))
        .await
        .expect("should receive init frame");

    assert_eq!(init["type"], "INIT", "first frame type must be INIT");
    assert!(
        init.get("line_statuses").is_some(),
        "init must have line_statuses"
    );
    assert!(
        init.get("active_line_index").is_some(),
        "init must have active_line_index"
    );
    assert!(
        init.get("trigger_count").is_some(),
        "init must have trigger_count"
    );
}

/// With mock telemetry, verify 10 Hz message rate (100 ms intervals).
#[tokio::test]
async fn ws_telemetry_rate_10hz() {
    let token = "test_token_rate";
    let app = make_test_state(token);

    // We need a mission loaded for mock telemetry to produce data.
    // Create mission via REST first.
    let (addr, _handle) = start_test_server(app, Some(15.0)).await.unwrap();

    // Create a mission so mock telemetry has waypoints to walk
    let client = Client::new();
    let base = format!("http://127.0.0.1:{}", addr.port());
    let mission_req = json!({
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
        .post(format!("{base}/api/v1/missions"))
        .bearer_auth(token)
        .json(&mission_req)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "mission create failed");

    // Give mock telemetry a moment to start producing
    sleep(Duration::from_millis(300)).await;

    let (_write, mut read) = ws_connect(addr, token).await;

    // Skip init frame
    let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
    assert_eq!(init["type"], "INIT");

    // Collect 10 TELEMETRY frames and measure elapsed time
    let start = Instant::now();
    let mut telem_count = 0;
    let deadline = Instant::now() + Duration::from_secs(5);

    while telem_count < 10 && Instant::now() < deadline {
        if let Some(msg) = next_text(&mut read, Duration::from_secs(2)).await {
            if msg["type"] == "TELEMETRY" {
                telem_count += 1;
            }
        } else {
            break;
        }
    }

    let elapsed = start.elapsed();
    assert_eq!(telem_count, 10, "should receive 10 telemetry frames");
    // 10 frames at 10 Hz = ~1 second. Allow generous tolerance for CI.
    assert!(
        elapsed < Duration::from_millis(3000),
        "10 frames should arrive within 3s, took {:?}",
        elapsed
    );
}

/// Disconnect, mutate state, reconnect — init frame reflects the mutation.
#[tokio::test]
async fn ws_reconnect_resync() {
    let token = "test_token_resync";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    // First connection — verify clean init
    {
        let (_write, mut read) = ws_connect(addr, token).await;
        let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
        assert_eq!(init["type"], "INIT");
        assert!(init["active_line_index"].is_null());
    }
    // Connection dropped (scope exit).

    // Mutate state while disconnected
    {
        let mut guard = fms.write().await;
        guard.last_status.active_line_index = Some(2);
        guard
            .last_status
            .line_statuses
            .push(irontrack::network::telemetry::LineStatus {
                completion_pct: 0.0,
                is_active: false,
            });
        guard
            .last_status
            .line_statuses
            .push(irontrack::network::telemetry::LineStatus {
                completion_pct: 0.0,
                is_active: false,
            });
        guard
            .last_status
            .line_statuses
            .push(irontrack::network::telemetry::LineStatus {
                completion_pct: 75.0,
                is_active: true,
            });
        let status_clone = guard.last_status.clone();
        let _ = guard.tx_status.send(status_clone);
    }

    // Reconnect — init frame should reflect the mutation
    {
        let (_write, mut read) = ws_connect(addr, token).await;
        let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
        assert_eq!(init["type"], "INIT");
        assert_eq!(init["active_line_index"], 2);
        assert_eq!(init["line_statuses"][2]["completion_pct"], 75.0);
        assert_eq!(init["line_statuses"][2]["is_active"], true);
    }
}

/// Inject a TriggerEvent and verify it arrives as a TRIGGER envelope.
#[tokio::test]
async fn ws_trigger_event_relay() {
    let token = "test_token_trigger";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (_write, mut read) = ws_connect(addr, token).await;

    // Skip init frame
    let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
    assert_eq!(init["type"], "INIT");

    // Inject a trigger event
    {
        let mut guard = fms.write().await;
        guard.record_trigger(TriggerEvent {
            timestamp_ms: 12345,
            line_index: 0,
            waypoint_index: 7,
            lat: 51.5,
            lon: -0.1,
            alt: 300.0,
        });
    }

    // Should receive TRIGGER envelope
    let trigger = next_text(&mut read, Duration::from_secs(5))
        .await
        .expect("should receive trigger event");
    assert_eq!(trigger["type"], "TRIGGER");
    assert_eq!(trigger["timestamp_ms"], 12345);
    assert_eq!(trigger["line_index"], 0);
    assert_eq!(trigger["waypoint_index"], 7);
    assert!((trigger["lat"].as_f64().unwrap() - 51.5).abs() < 1e-9);
}

/// Verify the server handles `RecvError::Lagged` gracefully.
///
/// Strategy: flood 300+ messages into the broadcast channel from a
/// background task while the client reader drains the WS stream.
/// The server handler will encounter `Lagged` on the broadcast side
/// (because the channel capacity is 256). The handler must not crash —
/// it should `continue` and eventually forward a sentinel message.
#[tokio::test]
async fn ws_lagged_receiver() {
    let token = "test_token_lag";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (_write, mut read) = ws_connect(addr, token).await;

    // Skip init frame
    let _ = next_text(&mut read, Duration::from_secs(5)).await;

    // Flood from a background task — we must send faster than the
    // handler can drain + forward, so we hold the sender and burst
    // everything synchronously.
    let fms_flood = fms.clone();
    tokio::spawn(async move {
        let guard = fms_flood.read().await;
        // Burst 300 messages (exceeds 256 capacity)
        for i in 0..300 {
            let _ = guard
                .tx_telemetry
                .send(irontrack::network::telemetry::CurrentState {
                    lat: 51.0 + (i as f64) * 0.0001,
                    lon: 0.0,
                    alt: 100.0,
                    alt_datum: irontrack::types::AltitudeDatum::Wgs84Ellipsoidal,
                    ground_speed_ms: 15.0,
                    true_heading_deg: 90.0,
                    fix_quality: 4,
                    ..Default::default()
                });
        }
        drop(guard);

        // Wait for the handler to process the lag, then send sentinel
        sleep(Duration::from_millis(300)).await;
        let guard = fms_flood.read().await;
        for _ in 0..5 {
            let _ = guard
                .tx_telemetry
                .send(irontrack::network::telemetry::CurrentState {
                    lat: 99.0,
                    lon: 99.0,
                    alt: 999.0,
                    alt_datum: irontrack::types::AltitudeDatum::Wgs84Ellipsoidal,
                    ground_speed_ms: 15.0,
                    true_heading_deg: 0.0,
                    fix_quality: 4,
                    ..Default::default()
                });
            sleep(Duration::from_millis(50)).await;
        }
    });

    // Read all messages — expect to eventually see the sentinel
    let mut found = false;
    for _ in 0..500 {
        match next_text(&mut read, Duration::from_millis(500)).await {
            Some(msg) if msg["type"] == "TELEMETRY" && msg["lat"] == 99.0 => {
                found = true;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }
    assert!(found, "lagged client should recover and receive fresh data");
}

/// Send malformed JSON and unknown message types — server should not crash.
#[tokio::test]
async fn ws_malformed_client_msg() {
    let token = "test_token_malformed";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (mut write, mut read) = ws_connect(addr, token).await;

    // Skip init frame
    let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
    assert_eq!(init["type"], "INIT");

    // Send garbage
    write
        .send(Message::Text("not json at all {{{{".to_string()))
        .await
        .unwrap();

    // Send unknown message type
    write
        .send(Message::Text(
            json!({"type": "UNKNOWN_TYPE", "data": 123}).to_string(),
        ))
        .await
        .unwrap();

    // Send empty object
    write.send(Message::Text("{}".to_string())).await.unwrap();

    // Small delay to let server process
    sleep(Duration::from_millis(200)).await;

    // Inject a trigger event to verify the connection is still alive
    {
        let mut guard = fms.write().await;
        guard.record_trigger(TriggerEvent {
            timestamp_ms: 99999,
            line_index: 0,
            waypoint_index: 0,
            lat: 0.0,
            lon: 0.0,
            alt: 0.0,
        });
    }

    // Should still receive data — connection alive after malformed input
    let msg = next_text(&mut read, Duration::from_secs(5)).await;
    assert!(
        msg.is_some(),
        "connection should survive malformed client messages"
    );
}

// ---------------------------------------------------------------------------
// Full daemon integration test (requires mission generation)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires full daemon environment and network setup"]
async fn test_daemon_full_flow() {
    // Set a known token so the test can authenticate.
    std::env::set_var("IRONTRACK_TOKEN", "integration_test_token_abc123");

    // 1. Start daemon in background (port 8081 to avoid conflict)
    let port = 8081;
    tokio::spawn(async move {
        irontrack::network::server::run_server(port, Some(15.0), 0, 0, 115_200, vec![])
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

    // 4. Verify initial frame (ServerMsg::Init envelope)
    let msg = ws_stream.next().await.unwrap().unwrap();
    if let Message::Text(text) = msg {
        let status: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(status["type"], "INIT");
        assert!(status.get("line_statuses").is_some());
    }

    // 5. Verify telemetry stream (ServerMsg::Telemetry envelope)
    // May need to skip STATUS messages from mock telemetry
    let mut found_telemetry = false;
    for _ in 0..20 {
        let msg = ws_stream.next().await.unwrap().unwrap();
        if let Message::Text(text) = msg {
            let val: Value = serde_json::from_str(&text).unwrap();
            if val["type"] == "TELEMETRY" {
                assert!(val.get("lat").is_some());
                found_telemetry = true;
                break;
            }
        }
    }
    assert!(found_telemetry, "should receive TELEMETRY frame");

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

    // 7. Verify WS receives STATUS update
    let mut found_update = false;
    for _ in 0..30 {
        let msg = ws_stream.next().await.unwrap().unwrap();
        if let Message::Text(text) = msg {
            let val: Value = serde_json::from_str(&text).unwrap();
            if val["type"] == "STATUS" {
                if let Some(statuses) = val.get("line_statuses") {
                    if statuses[0]["completion_pct"].as_f64().unwrap() == 50.0 {
                        found_update = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(found_update, "WS did not receive line update");
}
