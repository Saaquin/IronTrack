// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use rusqlite::Connection;
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use irontrack::network::server::{start_test_server, AppState, FmsState};
use irontrack::network::telemetry::{ServerMsg, TriggerEvent};

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

/// POST the standard small-bbox mission, return (mission_id, summary_json).
async fn create_mission(addr: std::net::SocketAddr, token: &str) -> (u64, Value) {
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
        .expect("mission POST failed");
    assert!(resp.status().is_success(), "mission create failed");
    let summary: Value = resp.json().await.expect("bad JSON from mission create");
    let id = summary["id"].as_u64().expect("mission id missing");
    (id, summary)
}

/// Read WS messages until `predicate` matches or `timeout` expires.
async fn wait_for_msg<F>(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout: Duration,
    predicate: F,
) -> Option<Value>
where
    F: Fn(&Value) -> bool,
{
    let deadline = tokio::time::timeout(timeout, async {
        loop {
            match read.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(val) = serde_json::from_str::<Value>(&text) {
                        if predicate(&val) {
                            return Some(val);
                        }
                    }
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

/// Remove mission.gpkg and its WAL/SHM side files for a clean slate.
fn cleanup_mission_gpkg() {
    for f in &["mission.gpkg", "mission.gpkg-wal", "mission.gpkg-shm"] {
        let _ = std::fs::remove_file(f);
    }
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
#[serial]
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

// ===========================================================================
// Phase 6D — Integration Test Gate (7 tests spanning 6A–6C)
// ===========================================================================

/// Test 1: Verify daemon boots with mock telemetry and serves an INIT frame.
#[tokio::test]
async fn integration_spawn_daemon_mock_telemetry() {
    let token = "token_spawn_mock";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, Some(15.0)).await.unwrap();

    assert!(addr.port() > 0, "server should bind a valid port");

    let (_write, mut read) = ws_connect(addr, token).await;
    let init = next_text(&mut read, Duration::from_secs(5))
        .await
        .expect("should receive INIT frame");

    assert_eq!(init["type"], "INIT");
    // No mission loaded yet, so line_statuses should be empty.
    assert_eq!(
        init["line_statuses"].as_array().unwrap().len(),
        0,
        "no mission yet — line_statuses should be empty"
    );
    assert_eq!(init["trigger_count"], 0);
}

/// Test 2: POST a boundary polygon, verify plan generation, GET lines.
#[tokio::test]
#[serial]
async fn integration_post_mission_verify_plan() {
    cleanup_mission_gpkg();

    let token = "token_post_mission";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, Some(15.0)).await.unwrap();

    let (id, summary) = create_mission(addr, token).await;

    // Validate MissionSummaryResponse fields
    assert!(id >= 1, "mission id should be >= 1");
    let line_count = summary["line_count"].as_u64().expect("line_count missing");
    assert!(line_count > 0, "should generate at least one flight line");
    let waypoint_count = summary["waypoint_count"]
        .as_u64()
        .expect("waypoint_count missing");
    assert!(
        waypoint_count > line_count,
        "each line has 2+ waypoints, so waypoint_count > line_count"
    );
    let total_dist = summary["total_distance_km"]
        .as_f64()
        .expect("total_distance_km missing");
    assert!(total_dist > 0.0, "total distance must be positive");
    let bbox = summary["bbox"].as_array().expect("bbox missing");
    assert_eq!(bbox.len(), 4, "bbox should have 4 elements");
    assert!(
        bbox[0].as_f64().unwrap() < bbox[2].as_f64().unwrap(),
        "min_lat < max_lat"
    );
    assert!(
        bbox[1].as_f64().unwrap() < bbox[3].as_f64().unwrap(),
        "min_lon < max_lon"
    );

    // GET mission by ID — should return the same summary
    let client = Client::new();
    let base = format!("http://127.0.0.1:{}", addr.port());
    let resp = client
        .get(format!("{base}/api/v1/missions/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let get_summary: Value = resp.json().await.unwrap();
    assert_eq!(get_summary["id"], id);
    assert_eq!(get_summary["line_count"], line_count);

    // GET lines as GeoJSON
    let resp = client
        .get(format!("{base}/api/v1/missions/{id}/lines"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let geojson: Value = resp.json().await.unwrap();
    assert_eq!(geojson["type"], "FeatureCollection");
    let features = geojson["features"].as_array().expect("features missing");
    assert_eq!(
        features.len(),
        line_count as usize,
        "GeoJSON feature count should match line_count"
    );
}

/// Test 3: After mission creation, WS INIT reflects state and 10 Hz stream flows.
#[tokio::test]
#[serial]
async fn integration_ws_init_and_telemetry_after_mission() {
    cleanup_mission_gpkg();

    let token = "token_ws_telem";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, Some(15.0)).await.unwrap();

    let (_id, summary) = create_mission(addr, token).await;
    let line_count = summary["line_count"].as_u64().unwrap() as usize;

    // Give mock telemetry a moment to start producing
    sleep(Duration::from_millis(300)).await;

    let (_write, mut read) = ws_connect(addr, token).await;

    // INIT frame should reflect loaded mission
    let init = next_text(&mut read, Duration::from_secs(5))
        .await
        .expect("should receive INIT frame");
    assert_eq!(init["type"], "INIT");
    let init_statuses = init["line_statuses"]
        .as_array()
        .expect("line_statuses missing");
    assert_eq!(
        init_statuses.len(),
        line_count,
        "INIT line_statuses.len() should match mission line_count"
    );

    // Collect 10 TELEMETRY frames
    let start = Instant::now();
    let mut telem_count = 0;
    let deadline = Instant::now() + Duration::from_secs(5);

    while telem_count < 10 && Instant::now() < deadline {
        if let Some(msg) = next_text(&mut read, Duration::from_secs(2)).await {
            if msg["type"] == "TELEMETRY" {
                // Validate fields on each frame
                assert!(msg["lat"].as_f64().is_some(), "lat must be a number");
                assert!(msg["lon"].as_f64().is_some(), "lon must be a number");
                assert!(msg["alt"].as_f64().is_some(), "alt must be a number");
                assert_eq!(
                    msg["fix_quality"], 4,
                    "mock telemetry fix_quality should be 4"
                );
                telem_count += 1;
            }
        } else {
            break;
        }
    }

    let elapsed = start.elapsed();
    assert_eq!(telem_count, 10, "should receive 10 TELEMETRY frames");
    assert!(
        elapsed < Duration::from_millis(3000),
        "10 frames should arrive within 3s, took {elapsed:?}"
    );
}

/// Test 4: Two WS clients both receive STATUS after PUT line update.
#[tokio::test]
#[serial]
async fn integration_put_line_ws_broadcast_multi_client() {
    cleanup_mission_gpkg();

    let token = "token_multi_ws";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, Some(15.0)).await.unwrap();

    let (id, _summary) = create_mission(addr, token).await;

    sleep(Duration::from_millis(200)).await;

    // Connect two WS clients and drain their INIT frames
    let (_write_a, mut read_a) = ws_connect(addr, token).await;
    let init_a = next_text(&mut read_a, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(init_a["type"], "INIT");

    let (_write_b, mut read_b) = ws_connect(addr, token).await;
    let init_b = next_text(&mut read_b, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(init_b["type"], "INIT");

    // PUT to update line 0 with a distinctive completion_pct
    let client = Client::new();
    let base = format!("http://127.0.0.1:{}", addr.port());
    let resp = client
        .put(format!("{base}/api/v1/missions/{id}/lines/0"))
        .bearer_auth(token)
        .json(&json!({ "completion_pct": 42.5, "is_active": true }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "PUT line should return 200");

    // Both clients should receive a STATUS with completion_pct == 42.5
    let predicate = |msg: &Value| {
        msg["type"] == "STATUS"
            && msg["line_statuses"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|s| s["completion_pct"].as_f64())
                == Some(42.5)
    };

    let status_a = wait_for_msg(&mut read_a, Duration::from_secs(5), predicate).await;
    assert!(
        status_a.is_some(),
        "client A should receive STATUS with completion_pct=42.5"
    );
    let sa = status_a.unwrap();
    assert_eq!(sa["line_statuses"][0]["is_active"], true);

    let status_b = wait_for_msg(&mut read_b, Duration::from_secs(5), predicate).await;
    assert!(
        status_b.is_some(),
        "client B should receive STATUS with completion_pct=42.5"
    );
}

/// Test 5: Inject RTK degradation WARNING, verify WS client receives it.
#[tokio::test]
async fn integration_rtk_degradation_warning() {
    let token = "token_rtk_warn";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (_write, mut read) = ws_connect(addr, token).await;
    let init = next_text(&mut read, Duration::from_secs(5)).await.unwrap();
    assert_eq!(init["type"], "INIT");

    // Inject a WARNING through the broadcast channel
    {
        let guard = fms.read().await;
        let _ = guard.tx_warning.send(ServerMsg::Warning {
            code: "RTK_DEGRADATION".to_string(),
            message: "RTK fix quality degraded from 4 to 2 (sustained 20 epochs)".to_string(),
            timestamp_ms: 12345,
        });
    }

    let warning = wait_for_msg(&mut read, Duration::from_secs(5), |msg| {
        msg["type"] == "WARNING"
    })
    .await;
    assert!(warning.is_some(), "should receive WARNING envelope");

    let w = warning.unwrap();
    assert_eq!(w["code"], "RTK_DEGRADATION");
    assert!(
        w["message"].as_str().unwrap().contains("degraded"),
        "message should contain 'degraded'"
    );
    assert_eq!(w["timestamp_ms"], 12345);
}

/// Test 6: Write completion via pool, abort server, reopen GeoPackage — WAL recovery.
#[tokio::test]
#[serial]
async fn integration_wal_recovery_after_crash() {
    cleanup_mission_gpkg();

    let token = "token_wal_recovery";
    let app = make_test_state(token);
    let fms = app.fms.clone();
    // No mock telemetry — we only need the REST API for mission creation.
    let (addr, handle) = start_test_server(app, None).await.unwrap();

    let (_id, summary) = create_mission(addr, token).await;
    let line_count = summary["line_count"].as_u64().unwrap();

    // Capture persistence path and pool before tearing down.
    let persistence_path = {
        let guard = fms.read().await;
        guard
            .persistence_path
            .clone()
            .expect("persistence_path should be set after mission creation")
    };

    // Write completion_pct = 75.0 for line 0 directly through the pool
    // (simulates what run_persistence_task does)
    {
        let guard = fms.read().await;
        let pool = guard
            .db_pool
            .clone()
            .expect("db_pool should be set after mission creation");
        drop(guard);

        let conn = pool.get().await.expect("pool.get() failed");
        conn.interact(move |conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; \
                 PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;",
            )
            .expect("pool PRAGMAs failed");
            conn.execute(
                "UPDATE flight_lines SET completion_pct = 75.0 WHERE line_index = 0",
                [],
            )
            .expect("UPDATE failed");
        })
        .await
        .expect("interact failed");

        // Do NOT run WAL checkpoint — simulate unclean shutdown.
    }

    // "Crash": abort the server task, then drop all state (including pool).
    handle.abort();
    let _ = handle.await; // wait for cancellation to complete
    drop(fms); // release the last Arc<FmsState>, dropping the pool

    // Small delay for OS file handle cleanup
    sleep(Duration::from_millis(100)).await;

    // Reopen the GeoPackage — SQLite WAL recovery replays the -wal file
    let gpkg_conn =
        Connection::open(&persistence_path).expect("should reopen GeoPackage after crash");
    gpkg_conn
        .execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; \
             PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;",
        )
        .expect("session PRAGMAs failed");

    // Validate application_id (GeoPackage magic)
    let app_id: i64 = gpkg_conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .expect("application_id query failed");
    assert_eq!(app_id, 1_196_444_487, "file must be a valid GeoPackage");

    // Verify completion_pct survived the crash
    let pct: f64 = gpkg_conn
        .query_row(
            "SELECT completion_pct FROM flight_lines WHERE line_index = 0",
            [],
            |r| r.get(0),
        )
        .expect("completion_pct query failed");
    assert!(
        (pct - 75.0).abs() < f64::EPSILON,
        "completion_pct should be 75.0 after WAL recovery, got {pct}"
    );

    // Verify all lines survived
    let count: i64 = gpkg_conn
        .query_row("SELECT count(*) FROM flight_lines", [], |r| r.get(0))
        .expect("count query failed");
    assert_eq!(
        count, line_count as i64,
        "all flight lines should survive WAL recovery"
    );
}

/// Test 7: GET /export/gpkg, validate binary is a well-formed GeoPackage.
#[tokio::test]
#[serial]
async fn integration_export_gpkg_valid() {
    cleanup_mission_gpkg();

    let token = "token_export_gpkg";
    let app = make_test_state(token);
    let (addr, _handle) = start_test_server(app, None).await.unwrap();

    let (id, summary) = create_mission(addr, token).await;
    let line_count = summary["line_count"].as_u64().unwrap();

    let client = Client::new();
    let base = format!("http://127.0.0.1:{}", addr.port());
    let resp = client
        .get(format!("{base}/api/v1/missions/{id}/export/gpkg"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success(), "export should return 200");

    // Validate headers
    let content_type = resp
        .headers()
        .get("content-type")
        .expect("Content-Type header missing")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("geopackage+sqlite3"),
        "Content-Type should be geopackage+sqlite3, got {content_type}"
    );
    let disposition = resp
        .headers()
        .get("content-disposition")
        .expect("Content-Disposition header missing")
        .to_str()
        .unwrap();
    assert!(
        disposition.contains("mission.gpkg"),
        "Content-Disposition should reference mission.gpkg"
    );

    let bytes = resp.bytes().await.expect("failed to read response body");
    assert!(bytes.len() > 100, "GeoPackage binary should be non-trivial");

    // SQLite magic: first 16 bytes = "SQLite format 3\0"
    assert_eq!(
        &bytes[0..16],
        b"SQLite format 3\0",
        "file must start with SQLite magic header"
    );

    // GeoPackage application_id at byte offset 68 (4 bytes, big-endian)
    let app_id = u32::from_be_bytes([bytes[68], bytes[69], bytes[70], bytes[71]]);
    assert_eq!(
        app_id, 0x47504B47,
        "application_id at offset 68 must be 0x47504B47"
    );

    // Write to a temp file and open with rusqlite to verify contents
    let tmp = tempfile::NamedTempFile::new().expect("tempfile creation failed");
    std::fs::write(tmp.path(), &bytes).expect("failed to write temp GeoPackage");

    let conn = Connection::open(tmp.path()).expect("should open exported GeoPackage");
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; \
         PRAGMA foreign_keys=ON; PRAGMA busy_timeout=0;",
    )
    .expect("session PRAGMAs failed");

    // Verify flight_lines table row count
    let count: i64 = conn
        .query_row("SELECT count(*) FROM flight_lines", [], |r| r.get(0))
        .expect("count query failed");
    assert_eq!(
        count, line_count as i64,
        "exported GeoPackage should have {line_count} flight lines"
    );

    // Verify distinct line indices
    let distinct: i64 = conn
        .query_row(
            "SELECT count(DISTINCT line_index) FROM flight_lines",
            [],
            |r| r.get(0),
        )
        .expect("distinct query failed");
    assert_eq!(
        distinct, line_count as i64,
        "each line should have a unique index"
    );

    // Verify default completion_pct
    let pct: f64 = conn
        .query_row(
            "SELECT completion_pct FROM flight_lines WHERE line_index = 0",
            [],
            |r| r.get(0),
        )
        .expect("completion_pct query failed");
    assert!(
        pct.abs() < f64::EPSILON,
        "completion_pct should default to 0.0 for new mission"
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
