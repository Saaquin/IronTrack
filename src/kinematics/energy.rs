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

//! Aerodynamic power model and per-segment energy costing for aerial survey
//! missions.
//!
//! Implements the simplified quadratic power model from Doc 38:
//!   P(V) = k1 × V³ + k2 / V   (parasite + induced drag)
//!
//! Key asymmetry: power is f(TAS) but time-on-segment is f(GS).
//! Headwind segments consume drastically more energy than tailwind segments.
//!
//! All functions are pure synchronous math — no async, no I/O.
//! Uses f64 exclusively for all physical quantities.
//!
//! Reference documents:
//!   - Doc 38: UAV Endurance Modeling — power models, battery, per-segment energy
//!   - Doc 50: Wind-corrected turn radius (r_safe)

use geographiclib_rs::{Geodesic, InverseGeodesic};
use rayon::prelude::*;

use crate::error::KinematicsError;
use crate::math::numerics::KahanAccumulator;
use crate::photogrammetry::flightlines::FlightPlan;

/// Standard gravity (m/s²).
const G: f64 = 9.806_65;

/// ISA sea-level air density (kg/m³).
const RHO_SEA_LEVEL: f64 = 1.225;

/// Usable fraction of battery capacity (20% reserve = use only 80%).
/// Doc 38 §3.4: the final 20% SoC exhibits a sharp voltage cliff that
/// virtually guarantees LVC breach under load.
const RESERVE_FRACTION: f64 = 0.80;

/// Watt-hours to joules conversion factor.
const WH_TO_J: f64 = 3600.0;

// ---------------------------------------------------------------------------
// PlatformDynamics
// ---------------------------------------------------------------------------

/// Aircraft platform parameters for aerodynamic power and energy estimation.
///
/// Doc 38 prescribed struct. Supports both multirotor (momentum theory) and
/// fixed-wing (drag polar) power models via optional wing fields.
///
/// The simplified quadratic model `P = k1·V³ + k2/V` is derived from these
/// fields. When `wing_area_m2`, `wingspan_m`, and `oswald_efficiency` are all
/// `Some`, the fixed-wing drag polar model is used instead of the multirotor
/// momentum theory model.
#[derive(Debug, Clone)]
pub struct PlatformDynamics {
    /// Total takeoff mass including payload (kg).
    pub mass_kg: f64,
    /// Parasite drag coefficient (dimensionless).
    pub drag_coefficient: f64,
    /// Frontal area perpendicular to flight direction (m²).
    pub frontal_area_m2: f64,
    /// Wing planform area (m²). Fixed-wing only.
    pub wing_area_m2: Option<f64>,
    /// Wingspan (m). Fixed-wing only, used to compute aspect ratio.
    pub wingspan_m: Option<f64>,
    /// Oswald efficiency factor (0.70–0.85 typical). Fixed-wing only.
    pub oswald_efficiency: Option<f64>,
    /// Total rotor disk area (m²). Multirotor only.
    pub rotor_disk_area_m2: Option<f64>,
    /// Propeller diameter (m). For advance ratio computation (future).
    pub propeller_diameter_m: Option<f64>,
    /// Battery capacity (Wh).
    pub battery_capacity_wh: f64,
    /// Peukert exponent (≈1.05 for LiPo). Always ≥ 1.0.
    pub battery_peukert_k: f64,
    /// Propulsive efficiency (0.0–1.0]. Motor + ESC + propeller combined.
    pub propulsive_efficiency: f64,
    /// Regenerative braking efficiency (0.0–0.10 typical). Energy recovered
    /// during descent as a fraction of gravitational potential energy.
    pub regen_efficiency: f64,
}

impl PlatformDynamics {
    /// Validated constructor. Returns `Err` if any parameter is out of range.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mass_kg: f64,
        drag_coefficient: f64,
        frontal_area_m2: f64,
        wing_area_m2: Option<f64>,
        wingspan_m: Option<f64>,
        oswald_efficiency: Option<f64>,
        rotor_disk_area_m2: Option<f64>,
        propeller_diameter_m: Option<f64>,
        battery_capacity_wh: f64,
        battery_peukert_k: f64,
        propulsive_efficiency: f64,
        regen_efficiency: f64,
    ) -> Result<Self, KinematicsError> {
        validate_positive_finite("mass_kg", mass_kg)?;
        validate_positive_finite("drag_coefficient", drag_coefficient)?;
        validate_positive_finite("frontal_area_m2", frontal_area_m2)?;
        validate_positive_finite("battery_capacity_wh", battery_capacity_wh)?;

        if !battery_peukert_k.is_finite() || battery_peukert_k < 1.0 {
            return Err(KinematicsError::InvalidParameter(format!(
                "battery_peukert_k must be >= 1.0 and finite, got {battery_peukert_k}"
            )));
        }
        if !propulsive_efficiency.is_finite()
            || propulsive_efficiency <= 0.0
            || propulsive_efficiency > 1.0
        {
            return Err(KinematicsError::InvalidParameter(format!(
                "propulsive_efficiency must be in (0.0, 1.0], got {propulsive_efficiency}"
            )));
        }
        if !regen_efficiency.is_finite() || !(0.0..=1.0).contains(&regen_efficiency) {
            return Err(KinematicsError::InvalidParameter(format!(
                "regen_efficiency must be in [0.0, 1.0], got {regen_efficiency}"
            )));
        }

        validate_optional_positive_finite("wing_area_m2", wing_area_m2)?;
        validate_optional_positive_finite("wingspan_m", wingspan_m)?;
        validate_optional_positive_finite("rotor_disk_area_m2", rotor_disk_area_m2)?;
        validate_optional_positive_finite("propeller_diameter_m", propeller_diameter_m)?;

        if let Some(e) = oswald_efficiency {
            if !e.is_finite() || e <= 0.0 || e > 1.0 {
                return Err(KinematicsError::InvalidParameter(format!(
                    "oswald_efficiency must be in (0.0, 1.0], got {e}"
                )));
            }
        }

        Ok(Self {
            mass_kg,
            drag_coefficient,
            frontal_area_m2,
            wing_area_m2,
            wingspan_m,
            oswald_efficiency,
            rotor_disk_area_m2,
            propeller_diameter_m,
            battery_capacity_wh,
            battery_peukert_k,
            propulsive_efficiency,
            regen_efficiency,
        })
    }

    /// DJI Phantom 4 Pro preset for testing and demo.
    ///
    /// Mass 1.4 kg, Cd 1.0, frontal area 0.04 m², rotor disk 0.12 m²,
    /// battery 89.2 Wh (5870 mAh × 15.2 V), Peukert 1.05, eta 0.65.
    pub fn phantom4pro() -> Self {
        Self {
            mass_kg: 1.4,
            drag_coefficient: 1.0,
            frontal_area_m2: 0.04,
            wing_area_m2: None,
            wingspan_m: None,
            oswald_efficiency: None,
            rotor_disk_area_m2: Some(0.12),
            propeller_diameter_m: Some(0.24),
            battery_capacity_wh: 89.2,
            battery_peukert_k: 1.05,
            propulsive_efficiency: 0.65,
            regen_efficiency: 0.0,
        }
    }

    /// Returns `true` if the platform has all fixed-wing parameters populated.
    fn is_fixed_wing(&self) -> bool {
        self.wing_area_m2.is_some() && self.wingspan_m.is_some() && self.oswald_efficiency.is_some()
    }

    /// Compute the parasite and induced drag coefficients (k1, k2) for the
    /// simplified quadratic power model: `P = k1·V³ + k2/V`.
    ///
    /// For fixed-wing:
    ///   k1 = 0.5·ρ·S·C_D0 / η
    ///   k2 = 2·W² / (ρ·π·b²·e·η)
    ///
    /// For multirotor:
    ///   k1 = 0.5·ρ·Cd·A_frontal / η
    ///   k2 = (m·g)² / (2·ρ·A_rotor·η)
    fn power_coefficients(&self) -> (f64, f64) {
        let eta = self.propulsive_efficiency;
        let weight = self.mass_kg * G;

        if self.is_fixed_wing() {
            let s = self
                .wing_area_m2
                .expect("is_fixed_wing() guarantees wing_area_m2 is Some");
            let b = self
                .wingspan_m
                .expect("is_fixed_wing() guarantees wingspan_m is Some");
            let e = self
                .oswald_efficiency
                .expect("is_fixed_wing() guarantees oswald_efficiency is Some");
            let k1 = 0.5 * RHO_SEA_LEVEL * s * self.drag_coefficient / eta;
            let k2 =
                2.0 * weight * weight / (RHO_SEA_LEVEL * std::f64::consts::PI * b * b * e * eta);
            (k1, k2)
        } else {
            let k1 = 0.5 * RHO_SEA_LEVEL * self.drag_coefficient * self.frontal_area_m2 / eta;
            let a_rotor = self.rotor_disk_area_m2.unwrap_or(0.12);
            let k2 = weight * weight / (2.0 * RHO_SEA_LEVEL * a_rotor * eta);
            (k1, k2)
        }
    }

    /// Power required for straight-and-level flight at `v_tas_ms` (Watts).
    ///
    /// Doc 38 §1.2/§2.2 simplified: `P = k1·V³ + k2/V`.
    pub fn power_required_w(&self, v_tas_ms: f64) -> Result<f64, KinematicsError> {
        if !v_tas_ms.is_finite() || v_tas_ms <= 0.0 {
            return Err(KinematicsError::InvalidParameter(format!(
                "TAS must be positive and finite, got {v_tas_ms}"
            )));
        }
        let (k1, k2) = self.power_coefficients();
        Ok(k1 * v_tas_ms.powi(3) + k2 / v_tas_ms)
    }

    /// Power required during a banked turn at `v_tas_ms` (Watts).
    ///
    /// Load factor `n = 1/cos(φ)`. Induced drag (k2 term) scales by n².
    /// Parasite drag (k1 term) is unchanged.
    ///
    /// Doc 38 §4.2: 60° bank → n=2.0 → 4× induced drag.
    pub fn power_in_turn_w(
        &self,
        v_tas_ms: f64,
        bank_angle_deg: f64,
    ) -> Result<f64, KinematicsError> {
        if !v_tas_ms.is_finite() || v_tas_ms <= 0.0 {
            return Err(KinematicsError::InvalidParameter(format!(
                "TAS must be positive and finite, got {v_tas_ms}"
            )));
        }
        if !bank_angle_deg.is_finite() || bank_angle_deg <= 0.0 || bank_angle_deg >= 90.0 {
            return Err(KinematicsError::InvalidParameter(format!(
                "bank angle must be in (0, 90) degrees, got {bank_angle_deg}"
            )));
        }

        let (k1, k2) = self.power_coefficients();
        let n = 1.0 / bank_angle_deg.to_radians().cos(); // load factor
        Ok(k1 * v_tas_ms.powi(3) + k2 * n * n / v_tas_ms)
    }

    /// Additive climb/descent power (Watts).
    ///
    /// Positive `v_z_ms` (climbing): `P_climb = m·g·Vz / η` (extra power).
    /// Negative `v_z_ms` (descending): `P_regen = -m·g·|Vz|·η_regen` (recovery).
    ///
    /// Doc 38 §4.3–4.4.
    pub fn climb_power_w(&self, v_z_ms: f64) -> f64 {
        let weight = self.mass_kg * G;
        if v_z_ms >= 0.0 {
            weight * v_z_ms / self.propulsive_efficiency
        } else {
            -weight * v_z_ms.abs() * self.regen_efficiency
        }
    }
}

// ---------------------------------------------------------------------------
// EnergyEstimate
// ---------------------------------------------------------------------------

/// Result of mission energy estimation.
///
/// Returned by [`estimate_mission_energy`] — not stored on `FlightPlan`
/// because energy is a derived analysis, not a property of the geometric plan.
#[derive(Debug, Clone)]
pub struct EnergyEstimate {
    /// Total energy for straight-and-level survey legs (J).
    pub survey_energy_j: f64,
    /// Total energy for transit/turn segments (J).
    pub turn_energy_j: f64,
    /// Net climb energy including regenerative recovery (J).
    pub climb_energy_j: f64,
    /// Grand total: survey + turn + climb (J).
    pub total_energy_j: f64,
    /// Battery capacity in joules (battery_wh × 3600).
    pub battery_capacity_j: f64,
    /// Total energy divided by reserve fraction (J).
    pub energy_with_reserve_j: f64,
    /// Whether the mission is feasible (`total <= 0.80 × capacity`).
    pub feasible: bool,
    /// Estimated endurance at average power draw (minutes).
    pub estimated_endurance_min: f64,
    /// Estimated mission time from segment distances and speeds (minutes).
    pub estimated_mission_time_min: f64,
}

// ---------------------------------------------------------------------------
// Per-segment energy
// ---------------------------------------------------------------------------

/// Compute energy cost of a single flight segment in joules.
///
/// Key asymmetry (Doc 38 §4.1): power is f(TAS) but time is f(GS).
/// `E = P(V_TAS) × (distance / V_GS)`
fn segment_energy_j(
    platform: &PlatformDynamics,
    distance_m: f64,
    v_tas_ms: f64,
    v_gs_ms: f64,
    is_turn: bool,
    bank_angle_deg: f64,
    delta_elev_m: f64,
) -> Result<f64, KinematicsError> {
    if distance_m <= 0.0 {
        return Ok(0.0);
    }

    let time_s = distance_m / v_gs_ms;

    let p_flight = if is_turn {
        platform.power_in_turn_w(v_tas_ms, bank_angle_deg)?
    } else {
        platform.power_required_w(v_tas_ms)?
    };

    let v_z = delta_elev_m / time_s;
    let p_climb = platform.climb_power_w(v_z);

    Ok((p_flight + p_climb) * time_s)
}

// ---------------------------------------------------------------------------
// Mission-level energy estimation
// ---------------------------------------------------------------------------

/// Compute total mission energy across all flight lines in a plan.
///
/// For each `FlightLine`, computes Karney geodesic distances per segment,
/// retrieves ground speeds from `FlightLine::ground_speeds()` (falls back to
/// `v_tas_ms` when wind has not been applied), and accumulates per-segment
/// energy with Kahan summation.
///
/// Transit lines (`is_transit == true`) use the banked-turn power model.
/// Elevation deltas from `FlightLine::elevations()` drive climb power.
///
/// # Arguments
/// - `plan`: flight plan with lines (may include transit/turn segments)
/// - `platform`: aircraft parameters for power model
/// - `v_tas_ms`: true airspeed in m/s (cruise speed)
/// - `bank_angle_deg`: maximum bank angle for turn segments
///
/// # Returns
/// `EnergyEstimate` with per-category energy, feasibility, and endurance.
pub fn estimate_mission_energy(
    plan: &FlightPlan,
    platform: &PlatformDynamics,
    v_tas_ms: f64,
    bank_angle_deg: f64,
) -> Result<EnergyEstimate, KinematicsError> {
    if !v_tas_ms.is_finite() || v_tas_ms <= 0.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "TAS must be positive and finite, got {v_tas_ms}"
        )));
    }
    if !bank_angle_deg.is_finite() || bank_angle_deg <= 0.0 || bank_angle_deg >= 90.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "bank angle must be in (0, 90) degrees, got {bank_angle_deg}"
        )));
    }

    let mut survey_acc = KahanAccumulator::new();
    let mut turn_acc = KahanAccumulator::new();
    let mut climb_acc = KahanAccumulator::new();
    let mut time_acc = KahanAccumulator::new();

    for line in &plan.lines {
        if line.len() < 2 {
            continue;
        }

        let lats = line.lats();
        let lons = line.lons();
        let elevations = line.elevations();
        let n_segments = line.len() - 1;

        // Compute geodesic distances via rayon (Karney).
        // InverseGeodesic<f64> returns s12 (distance in metres) directly.
        let distances: Vec<f64> = (0..n_segments)
            .into_par_iter()
            .map(|i| {
                let geod = Geodesic::wgs84();
                let lat1 = lats[i].to_degrees();
                let lon1 = lons[i].to_degrees();
                let lat2 = lats[i + 1].to_degrees();
                let lon2 = lons[i + 1].to_degrees();
                let dist: f64 = InverseGeodesic::inverse(&geod, lat1, lon1, lat2, lon2);
                dist
            })
            .collect();

        // Ground speeds: from wind triangle if available, else TAS.
        let gs_fallback: Vec<f64>;
        let ground_speeds = match line.ground_speeds() {
            Some(gs) => gs,
            None => {
                gs_fallback = vec![v_tas_ms; n_segments];
                &gs_fallback
            }
        };

        // Sequential Kahan accumulation per segment.
        for i in 0..n_segments {
            let dist = distances[i];
            let gs = ground_speeds[i];
            let delta_elev = elevations[i + 1] - elevations[i];

            let e_seg = segment_energy_j(
                platform,
                dist,
                v_tas_ms,
                gs,
                line.is_transit,
                bank_angle_deg,
                delta_elev,
            )?;

            // Separate climb energy for reporting.
            let time_s = if gs > 0.0 { dist / gs } else { 0.0 };
            let v_z = if time_s > 0.0 {
                delta_elev / time_s
            } else {
                0.0
            };
            let p_climb = platform.climb_power_w(v_z);
            let e_climb = p_climb * time_s;

            if line.is_transit {
                turn_acc.add(e_seg - e_climb);
            } else {
                survey_acc.add(e_seg - e_climb);
            }
            climb_acc.add(e_climb);
            time_acc.add(time_s);
        }
    }

    let survey_energy_j = survey_acc.total();
    let turn_energy_j = turn_acc.total();
    let climb_energy_j = climb_acc.total();
    let total_energy_j = survey_energy_j + turn_energy_j + climb_energy_j;
    let battery_capacity_j = platform.battery_capacity_wh * WH_TO_J;
    let usable_capacity_j = battery_capacity_j * RESERVE_FRACTION;
    let feasible = total_energy_j <= usable_capacity_j;

    let total_time_s = time_acc.total();
    let estimated_mission_time_min = total_time_s / 60.0;

    // Endurance = usable capacity / average power draw.
    let avg_power_w = if total_time_s > 0.0 {
        total_energy_j / total_time_s
    } else {
        // No segments — use level-flight power at TAS as fallback.
        platform.power_required_w(v_tas_ms)?
    };
    let estimated_endurance_min = if avg_power_w > 0.0 {
        usable_capacity_j / avg_power_w / 60.0
    } else {
        0.0
    };

    Ok(EnergyEstimate {
        survey_energy_j,
        turn_energy_j,
        climb_energy_j,
        total_energy_j,
        battery_capacity_j,
        energy_with_reserve_j: total_energy_j / RESERVE_FRACTION,
        feasible,
        estimated_endurance_min,
        estimated_mission_time_min,
    })
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_positive_finite(name: &str, value: f64) -> Result<(), KinematicsError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(KinematicsError::InvalidParameter(format!(
            "{name} must be positive and finite, got {value}"
        )));
    }
    Ok(())
}

fn validate_optional_positive_finite(
    name: &str,
    value: Option<f64>,
) -> Result<(), KinematicsError> {
    if let Some(v) = value {
        validate_positive_finite(name, v)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn test_platform() -> PlatformDynamics {
        PlatformDynamics::phantom4pro()
    }

    // -- PlatformDynamics validation --

    #[test]
    fn valid_platform_constructs() {
        let p = PlatformDynamics::new(
            1.4,
            1.0,
            0.04,
            None,
            None,
            None,
            Some(0.12),
            None,
            89.2,
            1.05,
            0.65,
            0.0,
        );
        assert!(p.is_ok());
    }

    #[test]
    fn negative_mass_rejected() {
        let p = PlatformDynamics::new(
            -1.0,
            1.0,
            0.04,
            None,
            None,
            None,
            Some(0.12),
            None,
            89.2,
            1.05,
            0.65,
            0.0,
        );
        assert!(p.is_err());
    }

    #[test]
    fn zero_efficiency_rejected() {
        let p = PlatformDynamics::new(
            1.4,
            1.0,
            0.04,
            None,
            None,
            None,
            Some(0.12),
            None,
            89.2,
            1.05,
            0.0,
            0.0,
        );
        assert!(p.is_err());
    }

    #[test]
    fn peukert_below_one_rejected() {
        let p = PlatformDynamics::new(
            1.4,
            1.0,
            0.04,
            None,
            None,
            None,
            Some(0.12),
            None,
            89.2,
            0.95,
            0.65,
            0.0,
        );
        assert!(p.is_err());
    }

    // -- Power model --

    #[test]
    fn power_at_zero_speed_rejected() {
        let p = test_platform();
        assert!(p.power_required_w(0.0).is_err());
        assert!(p.power_required_w(-5.0).is_err());
    }

    #[test]
    fn power_increases_with_speed_at_high_v() {
        let p = test_platform();
        // At high speed, parasite V^3 dominates — power should increase.
        let p10 = p.power_required_w(10.0).unwrap();
        let p20 = p.power_required_w(20.0).unwrap();
        let p30 = p.power_required_w(30.0).unwrap();
        assert!(p20 > p10);
        assert!(p30 > p20);
    }

    #[test]
    fn power_u_shaped_curve() {
        let p = test_platform();
        // The U-shaped curve has a minimum. Power at very low speed should be
        // higher than at moderate speed (induced drag dominates at low V).
        let p_low = p.power_required_w(2.0).unwrap();
        let p_mid = p.power_required_w(10.0).unwrap();
        let p_high = p.power_required_w(30.0).unwrap();
        // Mid should be lower than both extremes (U-shape).
        assert!(p_mid < p_low, "mid {p_mid} should be < low {p_low}");
        assert!(p_mid < p_high, "mid {p_mid} should be < high {p_high}");
    }

    #[test]
    fn turn_power_greater_than_straight() {
        let p = test_platform();
        let v = 15.0;
        let straight = p.power_required_w(v).unwrap();
        let turn_20 = p.power_in_turn_w(v, 20.0).unwrap();
        let turn_45 = p.power_in_turn_w(v, 45.0).unwrap();
        let turn_60 = p.power_in_turn_w(v, 60.0).unwrap();
        assert!(turn_20 > straight);
        assert!(turn_45 > turn_20);
        assert!(turn_60 > turn_45);
    }

    #[test]
    fn climb_power_positive_ascent() {
        let p = test_platform();
        let pc = p.climb_power_w(5.0); // 5 m/s climb
        assert!(pc > 0.0);
        // P_climb = m*g*Vz/eta = 1.4 * 9.80665 * 5.0 / 0.65 ≈ 105.6 W
        assert_abs_diff_eq!(pc, 1.4 * G * 5.0 / 0.65, epsilon = 0.1);
    }

    #[test]
    fn descent_with_regen_negative() {
        let mut p = test_platform();
        p.regen_efficiency = 0.067; // 6.7% recovery per Doc 38
        let pc = p.climb_power_w(-5.0); // descending
                                        // P_regen = -m*g*|Vz|*eta_regen = -(1.4 * 9.80665 * 5.0 * 0.067) ≈ -4.6 W
        assert!(pc < 0.0, "descent with regen should be negative power");
        assert_abs_diff_eq!(pc, -(1.4 * G * 5.0 * 0.067), epsilon = 0.1);
    }

    #[test]
    fn descent_without_regen_is_zero() {
        let p = test_platform(); // regen = 0.0
        let pc = p.climb_power_w(-5.0);
        assert_abs_diff_eq!(pc, 0.0, epsilon = 1e-10);
    }

    // -- Per-segment energy --

    #[test]
    fn zero_wind_energy_equals_power_times_d_over_v() {
        // Spec-required test: E = P(TAS) * d / TAS when GS = TAS.
        let p = test_platform();
        let v = 15.0;
        let d = 1000.0; // 1 km segment

        let e = segment_energy_j(&p, d, v, v, false, 20.0, 0.0).unwrap();

        let expected = p.power_required_w(v).unwrap() * (d / v);
        assert_abs_diff_eq!(e, expected, epsilon = 1e-6);
    }

    #[test]
    fn headwind_costs_more_than_tailwind() {
        // Spec-required test: headwind segment costs more energy.
        // Power is the same (f(TAS)), but headwind has lower GS → longer time.
        let p = test_platform();
        let v_tas = 30.0;
        let d = 1000.0;

        // Headwind: GS = TAS - wind = 20 m/s
        let e_headwind = segment_energy_j(&p, d, v_tas, 20.0, false, 20.0, 0.0).unwrap();
        // Tailwind: GS = TAS + wind = 40 m/s
        let e_tailwind = segment_energy_j(&p, d, v_tas, 40.0, false, 20.0, 0.0).unwrap();

        assert!(
            e_headwind > e_tailwind,
            "headwind energy {e_headwind} should exceed tailwind {e_tailwind}"
        );
        // Ratio should be GS_tail/GS_head = 40/20 = 2.0 (power cancels out).
        let ratio = e_headwind / e_tailwind;
        assert_abs_diff_eq!(ratio, 2.0, epsilon = 1e-6);
    }

    // -- Mission energy --

    #[test]
    fn feasibility_respects_20pct_reserve() {
        use crate::photogrammetry::flightlines::{
            generate_flight_lines, BoundingBox, FlightPlanParams,
        };
        use crate::types::SensorParams;

        let sensor = SensorParams {
            focal_length_mm: 50.0,
            sensor_width_mm: 53.4,
            sensor_height_mm: 40.0,
            image_width_px: 11664,
            image_height_px: 8750,
        };
        let params = FlightPlanParams::new(sensor, 0.05).unwrap();
        // Use a larger bbox to guarantee multi-waypoint flight lines.
        let bbox = BoundingBox {
            min_lat: 0.0,
            min_lon: 0.0,
            max_lat: 0.1,
            max_lon: 0.1,
        };
        let plan = generate_flight_lines(&bbox, 0.0, &params, None).unwrap();
        // Sanity: plan must have lines with actual segments.
        assert!(
            plan.lines.iter().any(|l| l.len() >= 2),
            "plan must contain multi-waypoint lines for this test"
        );

        // Tiny battery — should be infeasible.
        let mut platform = PlatformDynamics::phantom4pro();
        platform.battery_capacity_wh = 0.001; // nearly zero
        let est = estimate_mission_energy(&plan, &platform, 15.0, 20.0).unwrap();
        assert!(!est.feasible, "tiny battery should be infeasible");

        // Huge battery — should be feasible.
        platform.battery_capacity_wh = 100_000.0;
        let est = estimate_mission_energy(&plan, &platform, 15.0, 20.0).unwrap();
        assert!(est.feasible, "huge battery should be feasible");
    }
}
