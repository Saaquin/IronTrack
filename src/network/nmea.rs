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
    if let Some(lat) = parse_lat_lon(fields[2], fields[3]) {
        state.lat = lat;
    }

    // Longitude: Field 4 (dddmm.mmmm), Field 5 (E/W)
    if let Some(lon) = parse_lat_lon(fields[4], fields[5]) {
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

    // Latitude: Field 4
    if let Some(lat) = parse_lat_lon(fields[4], fields[5]) {
        state.lat = lat;
    }

    // Longitude: Field 6
    if let Some(lon) = parse_lat_lon(fields[6], fields[7]) {
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

fn parse_lat_lon(val: &str, dir: &str) -> Option<f64> {
    if val.is_empty() || dir.is_empty() {
        return None;
    }

    // Split degrees and minutes
    // Latitude: ddmm.mmmm -> 2 digits for degrees
    // Longitude: dddmm.mmmm -> 3 digits for degrees
    let dot_pos = val.find('.')?;
    let deg_len = dot_pos.saturating_sub(2);

    let (deg_part, min_part) = val.split_at(deg_len);
    let deg = deg_part.parse::<f64>().unwrap_or(0.0);
    let min = min_part.parse::<f64>().ok()?;

    let mut result = deg + (min / 60.0);
    if dir == "S" || dir == "W" {
        result = -result;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum() {
        // Checksum is XOR of all bytes between '$' and '*' (exclusive).
        // Computed: 0x53 for this GGA sentence.
        assert!(validate_checksum(
            "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*53"
        ));
        assert!(!validate_checksum(
            "$GNGGA,123519.00,4807.0381,N,01131.0001,E,4,12,0.9,545.4,M,46.9,M,1.5,0000*48"
        ));
    }

    #[test]
    fn test_parse_gga() {
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

    #[test]
    fn test_parse_rmc() {
        let mut state = CurrentState::default();
        // Checksum 0x37 computed for this RMC sentence.
        let sentence =
            "$GNRMC,210230.00,A,3855.4487,N,09446.0071,W,125.5,076.2,130495,003.8,E,A*37";
        assert!(NmeaParser::parse(sentence, &mut state));
        assert!((state.ground_speed_ms - 125.5 * 0.514444).abs() < 1e-5);
        assert_eq!(state.true_heading_deg, 76.2);
    }
}
