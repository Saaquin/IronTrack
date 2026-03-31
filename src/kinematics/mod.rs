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

//! Kinematics subsystem: wind triangle solver, wind-corrected turn radius,
//! atmospheric computations, and aerodynamic energy modeling for aerial survey
//! flight planning.
//!
//! All functions are pure synchronous math — no async, no I/O.
//! Uses f64 exclusively for all physical quantities.
//!
//! Reference documents:
//!   - Doc 03: Wind Correction Angle (WCA) / crab angle formulation
//!   - Doc 16: Aerial wind data, meteorological conventions
//!   - Doc 38: UAV Endurance Modeling — power models, per-segment energy
//!   - Doc 50: Wind-corrected turn radius (r_safe)

pub mod atmosphere;
pub mod energy;

pub use atmosphere::{WindResult, WindVector};
pub use energy::{EnergyEstimate, PlatformDynamics};
