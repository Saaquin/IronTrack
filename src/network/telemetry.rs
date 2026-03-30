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

use crate::types::AltitudeDatum;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CurrentState {
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub alt_datum: AltitudeDatum,
    pub ground_speed_ms: f64,
    pub true_heading_deg: f64,
    pub fix_quality: u8, // 0=None, 1=Autonomous, 2=DGPS, 4=RTK Fixed, 5=RTK Float
    pub hdop: f64,
    pub satellites_in_use: u8,
    pub geoid_separation_m: f64,
    pub utc_time: String, // "hhmmss.ss" from GGA/RMC field 1
    pub utc_date: String, // "ddmmyy" from RMC field 9
}

impl Default for CurrentState {
    fn default() -> Self {
        Self {
            lat: 0.0,
            lon: 0.0,
            alt: 0.0,
            alt_datum: AltitudeDatum::Wgs84Ellipsoidal,
            ground_speed_ms: 0.0,
            true_heading_deg: 0.0,
            fix_quality: 0,
            hdop: 99.9,
            satellites_in_use: 0,
            geoid_separation_m: 0.0,
            utc_time: String::new(),
            utc_date: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SystemStatus {
    pub active_line_index: Option<usize>,
    pub line_statuses: Vec<LineStatus>,
    pub trigger_count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LineStatus {
    pub completion_pct: f64,
    pub is_active: bool,
}

/// A single sensor trigger event recorded during flight execution.
///
/// Stored in a bounded in-memory ring buffer (`VecDeque`, 10 000 max)
/// and broadcast to WebSocket clients for real-time trigger confirmation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TriggerEvent {
    pub timestamp_ms: u64,
    pub line_index: usize,
    pub waypoint_index: usize,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
}

/// Type-discriminated envelope for all server-to-client WebSocket messages.
///
/// Each variant flattens into JSON with a `"type"` field so clients can
/// deterministically route messages without probing for field presence.
///
/// ```json
/// {"type": "TELEMETRY", "lat": 51.0, "lon": -0.1, ...}
/// {"type": "INIT", "active_line_index": null, "line_statuses": [], ...}
/// ```
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerMsg {
    /// Full state snapshot sent as the first frame on every new connection.
    Init(SystemStatus),
    /// 10 Hz kinematic telemetry (position, speed, heading, fix quality).
    Telemetry(CurrentState),
    /// State change notification (line selection, completion updates).
    Status(SystemStatus),
    /// Sensor trigger event (camera/LiDAR firing confirmation).
    Trigger(TriggerEvent),
    /// Operational warning broadcast (RTK degradation, DOP spike, etc.).
    Warning {
        code: String,
        message: String,
        timestamp_ms: u64,
    },
}
