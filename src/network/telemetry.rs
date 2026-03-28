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
