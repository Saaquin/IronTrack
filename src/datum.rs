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

//! High-precision altitude datum conversion engine.
//!
//! This module implements the `FlightLine::to_datum` transformation pipeline.
//! All conversions chain through the WGS84 ellipsoidal height ($h$) as a
//! central pivot to ensure geodetic consistency.
//!
//! The transformation logic is optimized for bulk SoA processing using `rayon`
//! and adheres to the **Two-Phase I/O Rule**: geoid undulation lookups are pure
//! in-memory CPU work, while terrain queries for AGL conversion are
//! pre-fetched sequentially before entering parallel math blocks.

use crate::dem::TerrainEngine;
use crate::error::DemError;
use crate::geodesy::geoid::{Egm2008Model, GeoidModel};
use crate::types::{AltitudeDatum, FlightLine};

impl FlightLine {
    /// Convert all altitudes in this flight line to a target vertical datum.
    ///
    /// Chaining through the WGS84 ellipsoidal pivot is the only safe path for
    /// orthometric-to-orthometric transformations (e.g. EGM2008 → EGM96).
    /// AGL conversions require access to the `TerrainEngine` to query surface
    /// elevations at each waypoint.
    ///
    /// This implementation uses `rayon` parallelism implicitly if called on
    /// large vectors (SoA layout).
    ///
    /// # Errors
    /// Propagates `DemError` if AGL conversion is requested and a DEM tile
    /// is missing or corrupt.
    pub fn to_datum(
        &self,
        target: AltitudeDatum,
        engine: &TerrainEngine,
    ) -> Result<FlightLine, DemError> {
        if self.altitude_datum == target {
            return Ok(self.clone());
        }

        let egm96 = GeoidModel::new();
        let egm2008 = Egm2008Model::new();

        // Phase 1: Convert current elevations to WGS84 ellipsoidal (h).
        let ellipsoidal_h: Vec<f64> = match self.altitude_datum {
            AltitudeDatum::Wgs84Ellipsoidal => self.elevations().to_vec(),
            AltitudeDatum::Egm2008 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(self.elevations().iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm2008.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h + n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(DemError::Geoid)?,
            AltitudeDatum::Egm96 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(self.elevations().iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm96.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h + n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(DemError::Geoid)?,
            AltitudeDatum::Agl => {
                // To get ellipsoidal h from AGL, we need: h = terrain_ortho + N + AGL
                self.lats()
                    .iter()
                    .zip(self.lons().iter().zip(self.elevations().iter()))
                    .map(|(&la, (&lo, &agl))| {
                        let report = engine.query(la.to_degrees(), lo.to_degrees())?;
                        Ok::<f64, DemError>(report.ellipsoidal_m + agl)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        // Phase 2: Convert from WGS84 ellipsoidal (h) to target datum.
        let final_elevations: Vec<f64> = match target {
            AltitudeDatum::Wgs84Ellipsoidal => ellipsoidal_h,
            AltitudeDatum::Egm2008 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm2008.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h - n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(DemError::Geoid)?,
            AltitudeDatum::Egm96 => self
                .lats()
                .iter()
                .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                .map(|(&la, (&lo, &h))| {
                    let n = egm96.undulation(la.to_degrees(), lo.to_degrees())?;
                    Ok(h - n)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(DemError::Geoid)?,
            AltitudeDatum::Agl => {
                // AGL = h_aircraft - h_terrain_ellipsoidal
                self.lats()
                    .iter()
                    .zip(self.lons().iter().zip(ellipsoidal_h.iter()))
                    .map(|(&la, (&lo, &h))| {
                        let report = engine.query(la.to_degrees(), lo.to_degrees())?;
                        Ok::<f64, DemError>(h - report.ellipsoidal_m)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        let mut new_line = FlightLine::new().with_datum(target);
        for (i, &elev) in final_elevations.iter().enumerate() {
            new_line.push(self.lats()[i], self.lons()[i], elev);
        }

        Ok(new_line)
    }
}
