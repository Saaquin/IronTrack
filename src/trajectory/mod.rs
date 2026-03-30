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

//! Terrain-following trajectory generation (v0.3).
//!
//! ## Pipeline
//!
//! ```text
//! Terrain Profile ──> Elastic Band ──> B-Spline Smooth ──> Pursuit ──> Waypoints
//!   (2D slice)        (energy min)     (C² continuous)     (look-ahead)  (autopilot)
//! ```
//!
//! All algorithms operate on a 2D vertical slice (along-track distance vs
//! altitude). The 3D geographic path is handled by the flight line generator;
//! this module only shapes the vertical profile.
//!
//! Reference: `docs/34_terrain_following_planning.md`

pub mod bspline_smooth;
pub mod elastic_band;
pub mod pursuit;
pub mod validation;

pub use bspline_smooth::BSplinePath;
pub use elastic_band::{ElasticBandSolver, FloorMode, SpringConstants};
pub use pursuit::PursuitController;
