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

// Copernicus attribution strings, DSM safety warning, and license constants.
//
// Copernicus DEM attribution is legally mandatory in any derived product.
// See docs/08_copernicusDEM.md for the full license analysis.
//
// The DSM safety warning is mandatory and non-suppressible. Copernicus
// GLO-30 is an X-band SAR Digital Surface Model. X-band radar penetrates
// 2-8 m into forest canopy depending on density and moisture, meaning AGL
// clearance over forests is UNDERESTIMATED when Copernicus is used as the
// terrain reference. See docs/09_DSMvDTM_AGL_Flight_Generation.md.

use std::io::IsTerminal;

// ---------------------------------------------------------------------------
// Attribution constants
// ---------------------------------------------------------------------------

/// Attribution template for the Copernicus DEM open-access license.
///
/// The `{year}` field must be substituted with the four-digit year of the
/// source data before use. Call [`copernicus_attribution()`] to get the
/// resolved string.
const COPERNICUS_ATTRIBUTION_TEMPLATE: &str =
    "Contains modified Copernicus Sentinel data {year}, processed by ESA.";

/// Default acquisition year for Copernicus GLO-30 tiles.
///
/// The current GLO-30 dataset is derived from Sentinel-1 IW mode data
/// acquired 2011-2015, released as the 2021 global product. When the
/// specific tile acquisition year is not available from tile metadata,
/// this default is used.
pub const COPERNICUS_DEFAULT_YEAR: u16 = 2021;

/// Build the resolved Copernicus attribution string for a given data year.
///
/// Replaces the `{year}` placeholder in the template with the actual year.
pub fn copernicus_attribution(year: u16) -> String {
    COPERNICUS_ATTRIBUTION_TEMPLATE.replace("{year}", &year.to_string())
}

/// Shorthand: attribution string using the default year (2021).
///
/// Use this when the specific tile acquisition year is not known.
pub fn copernicus_attribution_default() -> String {
    copernicus_attribution(COPERNICUS_DEFAULT_YEAR)
}

/// Disclaimer string identifying the Copernicus DEM license and access terms.
///
/// Accompanies the attribution in all derived output artefacts.
pub const COPERNICUS_DISCLAIMER: &str =
    "The Copernicus DEM is provided under the Copernicus \
     Data Space Ecosystem License. See \
     https://dataspace.copernicus.eu/terms-and-conditions";

/// Print the Copernicus attribution and disclaimer to stdout.
///
/// Called in the CLI summary output after the mission summary block and
/// before the DSM safety warning, so the operator sees both the legal
/// attribution and the operational warning in the correct order.
pub fn print_copernicus_attribution() {
    let attr = copernicus_attribution_default();
    println!("Attribution : {attr}");
    println!("License     : {COPERNICUS_DISCLAIMER}");
}

/// Plain-text body of the DSM safety warning, without box drawing or ANSI codes.
///
/// Used verbatim when embedding the warning in GeoPackage metadata and GeoJSON.
/// All consumer software must surface this text to the operator.
pub const DSM_WARNING_TEXT: &str =
    "SAFETY WARNING: Copernicus DEM is a Digital Surface Model (DSM). \
     Altitudes include canopy and structure tops. AGL clearance over forests \
     may be UNDERESTIMATED by 2-8 m. \
     Verify obstacle clearance manually before flight.";

/// Box-drawing form of the DSM safety warning, for terminal output.
const DSM_WARNING_BOX: &str = "\
\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}
\u{2551}  \u{26a0} SAFETY WARNING: COPERNICUS DEM IS A DSM                 \u{2551}
\u{2551}  Altitudes include canopy/structure tops. AGL clearance     \u{2551}
\u{2551}  over forests may be UNDERESTIMATED by 2-8 m.               \u{2551}
\u{2551}  Verify obstacle clearance manually before flight.          \u{2551}
\u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\
\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}";

/// Print the DSM safety warning to stderr.
///
/// If stderr is a TTY, the warning is wrapped in a box with ANSI bold/yellow
/// colouring to catch the operator's eye before flight. On a non-TTY stderr
/// (log file, CI output, pipe) the plain box-drawing text is emitted without
/// ANSI escape codes so it remains legible in plain-text logs.
///
/// This function must be called whenever a plan is produced from a DSM source.
/// It is intentionally not suppressible by any flag — the collision risk from
/// X-band canopy penetration requires mandatory disclosure.
pub fn print_dsm_warning() {
    if std::io::stderr().is_terminal() {
        /*
         * ANSI SGR 1;33 = bold + yellow. Reset (SGR 0) after the box so
         * subsequent output returns to the default colour.
         */
        eprintln!("\x1b[1;33m{DSM_WARNING_BOX}\x1b[0m");
    } else {
        eprintln!("{DSM_WARNING_BOX}");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_default_year_is_2021() {
        let attr = copernicus_attribution_default();
        assert!(
            attr.contains("2021"),
            "default attribution must contain year 2021, got: {attr}"
        );
        assert!(
            !attr.contains("{year}"),
            "placeholder must be substituted"
        );
    }

    #[test]
    fn attribution_custom_year() {
        let attr = copernicus_attribution(2024);
        assert!(attr.contains("2024"));
        assert!(!attr.contains("{year}"));
        assert!(attr.contains("Copernicus Sentinel data 2024"));
    }

    #[test]
    fn attribution_contains_esa() {
        let attr = copernicus_attribution_default();
        assert!(attr.contains("ESA"));
    }

    #[test]
    fn disclaimer_contains_license_url() {
        assert!(COPERNICUS_DISCLAIMER.contains("dataspace.copernicus.eu"));
    }

    #[test]
    fn dsm_warning_text_contains_safety() {
        assert!(DSM_WARNING_TEXT.contains("SAFETY WARNING"));
        assert!(DSM_WARNING_TEXT.contains("UNDERESTIMATED"));
    }
}
