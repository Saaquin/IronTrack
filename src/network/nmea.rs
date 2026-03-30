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

use crate::network::telemetry::CurrentState;
use crate::types::AltitudeDatum;

/// Intermediate GGA data held until a matching RMC completes the epoch.
struct GgaEpoch {
    utc_time: String,
    lat: f64,
    lon: f64,
    alt: f64,
    alt_datum: AltitudeDatum,
    fix_quality: u8,
    satellites_in_use: u8,
    hdop: f64,
    geoid_separation_m: f64,
}

/// Stateful parser for NMEA 0183 sentences.
///
/// Implements epoch consistency per Doc 23 §4.2: GGA is buffered until
/// a matching RMC (same UTC timestamp) arrives, then both are merged
/// into a single `CurrentState` update.  GGK sentences (Trimble
/// proprietary) are self-contained and bypass epoch pairing.
pub struct NmeaParser {
    pending_gga: Option<GgaEpoch>,
}

impl Default for NmeaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl NmeaParser {
    pub fn new() -> Self {
        Self { pending_gga: None }
    }

    /// Feed a single NMEA sentence.
    ///
    /// Returns `true` when `state` was updated with new data:
    /// - GGA: always `false` (buffered, awaiting matching RMC).
    /// - RMC: `true` — merges pending GGA if UTC matches, otherwise
    ///   applies kinematics only (speed, heading, date, time).
    /// - GGK: `true` — self-contained, updates position directly.
    pub fn parse(&mut self, sentence: &str, state: &mut CurrentState) -> bool {
        if !validate_checksum(sentence) {
            return false;
        }

        let body = sentence
            .trim_start_matches('$')
            .split('*')
            .next()
            .unwrap_or("");
        let fields: Vec<&str> = body.split(',').collect();
        if fields.is_empty() {
            return false;
        }

        // PTNL,GGK has a non-standard structure: fields[0]="PTNL", fields[1]="GGK"
        if fields[0] == "PTNL" && fields.len() > 1 && fields[1] == "GGK" {
            self.pending_gga = None;
            return parse_ggk(&fields, state);
        }

        // Standard NMEA: fields[0] = <2-char talker><3-char formatter>
        if fields[0].len() < 5 {
            return false;
        }
        let formatter = &fields[0][2..];

        match formatter {
            "GGA" => {
                self.pending_gga = extract_gga_epoch(&fields);
                false
            }
            "RMC" => self.try_complete_epoch(&fields, state),
            _ => false,
        }
    }

    /// Attempt to complete an epoch by merging pending GGA with this RMC.
    fn try_complete_epoch(&mut self, fields: &[&str], state: &mut CurrentState) -> bool {
        if !apply_rmc_kinematics(fields, state) {
            return false;
        }

        let rmc_utc = if fields.len() > 1 && !fields[1].is_empty() {
            fields[1]
        } else {
            ""
        };

        // If pending GGA matches on UTC, merge position data into state.
        if let Some(gga) = self.pending_gga.take() {
            if !rmc_utc.is_empty() && gga.utc_time == rmc_utc {
                state.lat = gga.lat;
                state.lon = gga.lon;
                state.alt = gga.alt;
                state.alt_datum = gga.alt_datum;
                state.fix_quality = gga.fix_quality;
                state.satellites_in_use = gga.satellites_in_use;
                state.hdop = gga.hdop;
                state.geoid_separation_m = gga.geoid_separation_m;
            }
            // Stale GGA (UTC mismatch) is silently consumed.
        }

        true
    }
}

fn validate_checksum(sentence: &str) -> bool {
    if !sentence.starts_with('$') {
        return false;
    }
    let parts: Vec<&str> = sentence.split('*').collect();
    if parts.len() != 2 {
        return false;
    }

    let body = &parts[0][1..];
    let expected_checksum_str = parts[1].trim_end();
    if expected_checksum_str.len() < 2 {
        return false;
    }

    let mut checksum: u8 = 0;
    for b in body.as_bytes() {
        checksum ^= b;
    }

    if let Ok(expected) = u8::from_str_radix(&expected_checksum_str[0..2], 16) {
        checksum == expected
    } else {
        false
    }
}

/// Extract GGA fields into an intermediate epoch buffer.
fn extract_gga_epoch(fields: &[&str]) -> Option<GgaEpoch> {
    if fields.len() < 12 {
        return None;
    }

    let utc_time = if !fields[1].is_empty() {
        fields[1].to_string()
    } else {
        return None; // Cannot pair without UTC
    };

    // Reject the entire epoch if critical fields are missing or unparseable.
    // Altitude and geoid separation are safety-critical — a silent 0.0 default
    // is indistinguishable from a legitimate sea-level reading. [Audit F2]
    // Non-critical fields (HDOP, satellites) fall back to safe defaults.
    let lat = parse_nmea_coordinate(fields[2], fields[3], 2)?;
    let lon = parse_nmea_coordinate(fields[4], fields[5], 3)?;
    let fix_quality = fields[6].parse::<u8>().ok()?;
    let satellites_in_use = fields[7].parse::<u8>().unwrap_or(0);
    let hdop = fields[8].parse::<f64>().unwrap_or(99.9);
    let alt = fields[9].parse::<f64>().ok()?;
    let geoid_separation_m = fields[11].parse::<f64>().ok()?;

    Some(GgaEpoch {
        utc_time,
        lat,
        lon,
        alt,
        alt_datum: AltitudeDatum::Egm96,
        fix_quality,
        satellites_in_use,
        hdop,
        geoid_separation_m,
    })
}

/// Apply RMC kinematic fields (speed, heading, time, date) to state.
/// Returns `false` if the sentence is malformed or status is invalid.
fn apply_rmc_kinematics(fields: &[&str], state: &mut CurrentState) -> bool {
    if fields.len() < 9 {
        return false;
    }

    // Status: Field 2 (A=Valid, V=Warning)
    if fields[2] != "A" {
        return false;
    }

    // UTC time: Field 1 (hhmmss.ss)
    if !fields[1].is_empty() {
        state.utc_time = fields[1].to_string();
    }

    // Ground Speed (knots): Field 7
    if let Ok(knots) = fields[7].parse::<f64>() {
        state.ground_speed_ms = knots * 0.514444; // knots to m/s
    }

    // Course Over Ground (degrees true): Field 8
    if let Ok(cog) = fields[8].parse::<f64>() {
        state.true_heading_deg = cog;
    }

    // UTC date: Field 9 (ddmmyy)
    if fields.len() > 9 && !fields[9].is_empty() {
        state.utc_date = fields[9].to_string();
    }

    true
}

fn parse_ggk(fields: &[&str], state: &mut CurrentState) -> bool {
    // fields[0] is PTNL, fields[1] is GGK
    if fields.len() < 12 {
        return false;
    }

    // Latitude: Field 4 (ddmm.mmmm)
    if let Some(lat) = parse_nmea_coordinate(fields[4], fields[5], 2) {
        state.lat = lat;
    }

    // Longitude: Field 6 (dddmm.mmmm)
    if let Some(lon) = parse_nmea_coordinate(fields[6], fields[7], 3) {
        state.lon = lon;
    }

    // Trimble RTK Quality: Field 8 (3=Fixed, 2=Float)
    if let Ok(q) = fields[8].parse::<u8>() {
        state.fix_quality = match q {
            3 => 4, // Map Trimble Fixed to standard GGA Fixed
            2 => 5, // Map Trimble Float to standard GGA Float
            _ => q,
        };
    }

    // Ellipsoidal height: Field 11 (prefixed with EHT)
    let eht_str = fields[11].trim_start_matches("EHT");
    if let Ok(eht) = eht_str.parse::<f64>() {
        state.alt = eht;
        state.alt_datum = AltitudeDatum::Wgs84Ellipsoidal;
    }

    true
}

/// Parse an NMEA coordinate field into decimal degrees.
///
/// `deg_len` is the fixed number of degree digits in the NMEA format:
/// - 2 for latitude  (ddmm.mmmm)
/// - 3 for longitude (dddmm.mmmm)
///
/// Returns `None` on any parse failure — never defaults to 0.0.
fn parse_nmea_coordinate(val: &str, dir: &str, deg_len: usize) -> Option<f64> {
    if val.is_empty() || dir.is_empty() {
        return None;
    }

    // Reject invalid hemisphere markers
    if !matches!(dir, "N" | "S" | "E" | "W") {
        return None;
    }

    // Reject inputs too short to contain degrees + at least "mm" for minutes
    if val.len() < deg_len + 2 {
        return None;
    }

    let (deg_part, min_part) = val.split_at(deg_len);
    let deg = deg_part.parse::<f64>().ok()?;
    let min = min_part.parse::<f64>().ok()?;

    if !(0.0..60.0).contains(&min) {
        return None;
    }

    let mut result = deg + (min / 60.0);
    if dir == "S" || dir == "W" {
        result = -result;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Checksum validation
    // ------------------------------------------------------------------

    #[test]
    fn checksum_valid() {
        assert!(validate_checksum(
            "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53"
        ));
    }

    #[test]
    fn checksum_invalid_rejects_corrupted() {
        assert!(!validate_checksum(
            "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*48"
        ));
    }

    #[test]
    fn checksum_missing_dollar_sign() {
        assert!(!validate_checksum("GNGGA,123519.00*53"));
    }

    #[test]
    fn checksum_missing_asterisk() {
        assert!(!validate_checksum("$GNGGA,123519.00"));
    }

    // ------------------------------------------------------------------
    // parse_nmea_coordinate — unit tests
    // ------------------------------------------------------------------

    #[test]
    fn coord_standard_latitude() {
        // "4807.038" with deg_len=2 → degrees=48, minutes=07.038 → 48 + 7.038/60
        let result = parse_nmea_coordinate("4807.038", "N", 2).unwrap();
        assert!((result - 48.1173).abs() < 1e-4);
    }

    #[test]
    fn coord_standard_longitude() {
        // "01131.000" with deg_len=3 → degrees=011, minutes=31.000 → 11.51667
        let result = parse_nmea_coordinate("01131.000", "E", 3).unwrap();
        assert!((result - 11.51667).abs() < 1e-4);
    }

    #[test]
    fn coord_low_degree_longitude() {
        // "00530.000" → degrees=005, minutes=30.000 → 5.5
        let result = parse_nmea_coordinate("00530.000", "E", 3).unwrap();
        assert!((result - 5.5).abs() < 1e-7);
    }

    #[test]
    fn coord_180_longitude() {
        // "18000.000" → degrees=180, minutes=00.000 → 180.0
        let result = parse_nmea_coordinate("18000.000", "E", 3).unwrap();
        assert!((result - 180.0).abs() < 1e-7);
    }

    #[test]
    fn coord_south_negates() {
        let result = parse_nmea_coordinate("3330.000", "S", 2).unwrap();
        assert!((result - -33.5).abs() < 1e-7);
    }

    #[test]
    fn coord_west_negates() {
        let result = parse_nmea_coordinate("09446.0071", "W", 3).unwrap();
        assert!(result < 0.0);
    }

    #[test]
    fn coord_malformed_too_short_returns_none() {
        // "1.5" is too short for deg_len=2 (needs at least 2+2=4 chars)
        assert!(parse_nmea_coordinate("1.5", "N", 2).is_none());
    }

    #[test]
    fn coord_malformed_minutes_ge_60_returns_none() {
        // "4860.000" → degrees=48, minutes=60.000 — invalid
        assert!(parse_nmea_coordinate("4860.000", "N", 2).is_none());
    }

    #[test]
    fn coord_empty_value_returns_none() {
        assert!(parse_nmea_coordinate("", "N", 2).is_none());
    }

    #[test]
    fn coord_empty_direction_returns_none() {
        assert!(parse_nmea_coordinate("4807.038", "", 2).is_none());
    }

    #[test]
    fn coord_non_numeric_returns_none() {
        assert!(parse_nmea_coordinate("ABCD.EFG", "N", 2).is_none());
    }

    #[test]
    fn coord_invalid_hemisphere_returns_none() {
        assert!(parse_nmea_coordinate("4807.038", "X", 2).is_none());
    }

    // ------------------------------------------------------------------
    // Full GGA sentence parsing
    // ------------------------------------------------------------------

    #[test]
    fn parse_gga_buffers_without_updating_state() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let gga = "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53";
        // GGA alone buffers — does NOT update state
        assert!(!parser.parse(gga, &mut state));
        assert_eq!(state.lat, 0.0);
    }

    #[test]
    fn parse_gga_rmc_epoch_match() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let gga = "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53";
        let rmc = "$GNRMC,123519.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*38";
        assert!(!parser.parse(gga, &mut state));
        assert!(parser.parse(rmc, &mut state));
        // GGA position merged
        assert_eq!(state.fix_quality, 4);
        assert!((state.lat - 48.1173016).abs() < 1e-7);
        assert!((state.lon - 11.5166683).abs() < 1e-7);
        assert_eq!(state.alt, 545.4);
        assert_eq!(state.alt_datum, AltitudeDatum::Egm96);
        // RMC kinematics merged
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.true_heading_deg, 76.2);
    }

    // ------------------------------------------------------------------
    // Full RMC sentence parsing (kinematic-only, no preceding GGA)
    // ------------------------------------------------------------------

    #[test]
    fn parse_rmc_kinematic_only() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let rmc = "$GNRMC,210230.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*37";
        assert!(parser.parse(rmc, &mut state));
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.true_heading_deg, 76.2);
        // Position remains at default (no GGA to merge)
        assert_eq!(state.lat, 0.0);
    }

    // ------------------------------------------------------------------
    // Epoch consistency
    // ------------------------------------------------------------------

    #[test]
    fn epoch_mismatch_discards_stale_gga() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        // GGA at UTC 100000.00
        let gga = "$GNGGA,100000.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*5F";
        // RMC at UTC 100001.00 (mismatched)
        let rmc = "$GNRMC,100001.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*35";
        assert!(!parser.parse(gga, &mut state));
        assert!(parser.parse(rmc, &mut state));
        // Lat/lon remain at default — stale GGA discarded
        assert_eq!(state.lat, 0.0);
        assert_eq!(state.lon, 0.0);
        // RMC kinematics applied
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.utc_date, "130495");
    }

    #[test]
    fn parse_rmc_extracts_date() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let rmc = "$GNRMC,210230.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*37";
        assert!(parser.parse(rmc, &mut state));
        assert_eq!(state.utc_date, "130495");
        assert_eq!(state.utc_time, "210230.00");
    }

    #[test]
    fn parse_gga_extracts_hdop_sats_geoid() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let gga = "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53";
        let rmc = "$GNRMC,123519.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*38";
        assert!(!parser.parse(gga, &mut state));
        assert!(parser.parse(rmc, &mut state));
        assert_eq!(state.satellites_in_use, 12);
        assert!((state.hdop - 0.9).abs() < 1e-6);
        assert!((state.geoid_separation_m - 46.9).abs() < 1e-6);
        assert_eq!(state.utc_time, "123519.00");
    }

    // ------------------------------------------------------------------
    // GGK (Trimble proprietary)
    // ------------------------------------------------------------------

    #[test]
    fn parse_ggk_trimble_fixed() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let ggk =
            "$PTNL,GGK,102939.00,051910,5000.97323841,N,00827.62010742,E,3,09,1.9,EHT150.790,M*75";
        assert!(parser.parse(ggk, &mut state));
        assert_eq!(state.fix_quality, 4); // Trimble 3 → standard 4
        assert_eq!(state.alt_datum, AltitudeDatum::Wgs84Ellipsoidal);
        assert!((state.alt - 150.790).abs() < 1e-3);
        // 8-decimal precision: 5000.97323841 → 50 + 0.97323841/60 ≈ 50.01622064
        assert!((state.lat - 50.01622064).abs() < 1e-7);
    }

    #[test]
    fn parse_ggk_does_not_require_rmc_pairing() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        let ggk =
            "$PTNL,GGK,102939.00,051910,5000.97323841,N,00827.62010742,E,3,09,1.9,EHT150.790,M*75";
        // Single GGK returns true and updates position directly
        assert!(parser.parse(ggk, &mut state));
        assert!(state.lat != 0.0);
        assert!(state.lon != 0.0);
    }

    #[test]
    fn ggk_clears_pending_gga() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        // Buffer a GGA first
        let gga = "$GNGGA,102939.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*5E";
        assert!(!parser.parse(gga, &mut state));
        // GGK clears the pending GGA
        let ggk =
            "$PTNL,GGK,102939.00,051910,5000.97323841,N,00827.62010742,E,3,09,1.9,EHT150.790,M*75";
        assert!(parser.parse(ggk, &mut state));
        // Now RMC with matching UTC should NOT merge stale GGA position
        let rmc = "$GNRMC,102939.00,A,5000.9732,N,00827.6201,E,55.0,180.5,290326,003.1,E,R*08";
        assert!(parser.parse(rmc, &mut state));
        // Position should remain from GGK (lat ≈ 50.016), not GGA (lat ≈ 48.117)
        assert!((state.lat - 50.01622064).abs() < 1e-5);
    }

    #[test]
    fn rmc_without_gga_updates_kinematics_only() {
        let mut parser = NmeaParser::new();
        let mut state = CurrentState::default();
        state.lat = 51.0; // Pre-set position
        state.lon = -1.0;
        let rmc = "$GNRMC,210230.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*37";
        assert!(parser.parse(rmc, &mut state));
        // Position unchanged (no GGA to merge)
        assert_eq!(state.lat, 51.0);
        assert_eq!(state.lon, -1.0);
        // Kinematics updated
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.true_heading_deg, 76.2);
    }
}
