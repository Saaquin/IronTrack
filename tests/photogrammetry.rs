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

//! Integration tests for the photogrammetry subsystem.
//!
//! Reference sensor: DJI Phantom 4 Pro (focal=8.8mm, sensor_w=13.2mm,
//! sensor_h=8.8mm, 5472×3648 px). Published GSD tables from DJI and
//! Pix4D serve as the ground truth for formula validation.

// stub: uncomment and implement when src/photogrammetry/ is populated
// use irontrack::photogrammetry;

/// GSD formula: (AGL_m × pixel_pitch_mm / focal_length_mm) × 100 = GSD cm/px.
/// Phantom 4 Pro at 120 m AGL → GSD ≈ 3.29 cm/px.
/// Assert within 0.01 cm/px of published reference.
#[test]
#[ignore = "stub: implement when sensor GSD is complete"]
fn gsd_phantom4pro_120m_agl() {}

/// GSD must scale linearly with AGL. Doubling AGL must exactly double GSD.
#[test]
#[ignore = "stub: implement when sensor GSD is complete"]
fn gsd_scales_linearly_with_agl() {}

/// Swath width: 2 × AGL × tan(HFOV / 2).
/// HFOV = 2 × atan(sensor_width / (2 × focal_length)).
/// Assert swath width within 0.01 m of hand-calculated value.
#[test]
#[ignore = "stub: implement when swath width is complete"]
fn swath_width_phantom4pro_120m_agl() {}

/// Flight line spacing = swath_width × (1 - side_overlap).
/// At 70% side overlap with a 200 m swath: spacing = 60 m.
#[test]
#[ignore = "stub: implement when flight line spacing is complete"]
fn flight_line_spacing_from_side_overlap() {}

/// Overlap values in [0.0, 1.0) must succeed without error.
#[test]
#[ignore = "stub: implement when overlap validation is complete"]
fn valid_overlap_values_succeed() {}

/// Overlap values outside [0.0, 1.0) must return PhotogrammetryError::InvalidOverlap.
#[test]
#[ignore = "stub: implement when overlap validation is complete"]
fn invalid_overlap_out_of_range_returns_error() {}

/// Dynamic AGL: given a terrain profile rising 50 m over the mission area,
/// the flight altitude above MSL must increase by 50 m to maintain constant AGL.
/// The resulting GSD must remain constant across the profile.
#[test]
#[ignore = "stub: implement when dynamic AGL is complete"]
fn dynamic_agl_adjusts_for_terrain_rise() {}

/// AGL below minimum terrain clearance must return PhotogrammetryError::InsufficientAgl.
#[test]
#[ignore = "stub: implement when dynamic AGL is complete"]
fn agl_below_terrain_clearance_returns_error() {}
