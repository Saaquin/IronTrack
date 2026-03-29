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

/// Parser for NMEA 0183 sentences.
pub struct NmeaParser;

impl NmeaParser {
    /// Validate checksum and parse a single NMEA sentence.
    /// Updates the provided `CurrentState` with any fields found.
    pub fn parse(sentence: &str, state: &mut CurrentState) -> bool {
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

        let _talker_id = &fields[0][0..2];
        let formatter = &fields[0][2..];

        match formatter {
            "GGA" => parse_gga(&fields, state),
            "RMC" => parse_rmc(&fields, state),
            "GGK" if fields[0] == "PTNL" => parse_ggk(&fields, state),
            _ => false,
        }
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

fn parse_gga(fields: &[&str], state: &mut CurrentState) -> bool {
    if fields.len() < 12 {
        return false;
    }

    // Latitude: Field 2 (ddmm.mmmm), Field 3 (N/S)
    if let Some(lat) = parse_nmea_coordinate(fields[2], fields[3], 2) {
        state.lat = lat;
    }

    // Longitude: Field 4 (dddmm.mmmm), Field 5 (E/W)
    if let Some(lon) = parse_nmea_coordinate(fields[4], fields[5], 3) {
        state.lon = lon;
    }

    // Fix Quality: Field 6
    if let Ok(quality) = fields[6].parse::<u8>() {
        state.fix_quality = quality;
    }

    // Orthometric height (MSL): Field 9
    if let Ok(msl) = fields[9].parse::<f64>() {
        // We track altitude in MSL (EGM96 for most NMEA)
        state.alt = msl;
        state.alt_datum = AltitudeDatum::Egm96;
    }

    true
}

fn parse_rmc(fields: &[&str], state: &mut CurrentState) -> bool {
    if fields.len() < 9 {
        return false;
    }

    // Status: Field 2 (A=Valid, V=Warning)
    if fields[2] != "A" {
        return false;
    }

    // Ground Speed (knots): Field 7
    if let Ok(knots) = fields[7].parse::<f64>() {
        state.ground_speed_ms = knots * 0.514444; // knots to m/s
    }

    // Course Over Ground (degrees true): Field 8
    if let Ok(cog) = fields[8].parse::<f64>() {
        state.true_heading_deg = cog;
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
    fn parse_gga_standard() {
        let mut state = CurrentState::default();
        let sentence =
            "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53";
        assert!(NmeaParser::parse(sentence, &mut state));
        assert_eq!(state.fix_quality, 4);
        assert!((state.lat - 48.1173016).abs() < 1e-7);
        assert!((state.lon - 11.5166683).abs() < 1e-7);
        assert_eq!(state.alt, 545.4);
        assert_eq!(state.alt_datum, AltitudeDatum::Egm96);
    }

    // ------------------------------------------------------------------
    // Full RMC sentence parsing
    // ------------------------------------------------------------------

    #[test]
    fn parse_rmc_standard() {
        let mut state = CurrentState::default();
        let sentence =
            "$GNRMC,210230.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*37";
        assert!(NmeaParser::parse(sentence, &mut state));
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.true_heading_deg, 76.2);
    }
}
