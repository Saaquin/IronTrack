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

// GeoPackage subsystem: SQLite database creation (WAL mode), StandardGeoPackageBinary
// header + WKB encoding, R-tree virtual table, and the 7 synchronisation triggers.

pub mod binary;
pub mod init;
pub mod rtree;

pub use binary::encode_linestring;
pub use init::GeoPackage;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn new_gpkg_has_wal_mode() {
        // Covered in detail by init::tests::new_gpkg_has_wal_mode.
        // Verified here at the module boundary via the public GeoPackage type.
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.gpkg");
        let gpkg = GeoPackage::new(&path).unwrap();
        let mode: String = gpkg
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn binary_header_magic_gp() {
        let blob = encode_linestring(&[(0.0, 51.5), (0.1, 51.5)], 4326);
        assert_eq!(blob[0], 0x47, "magic byte 0 must be 0x47 ('G')");
        assert_eq!(blob[1], 0x50, "magic byte 1 must be 0x50 ('P')");
    }

    #[test]
    fn rtree_populated_after_geometry_insert() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rtree_test.gpkg");
        let gpkg = GeoPackage::new(&path).unwrap();
        gpkg.create_feature_table("flight_lines").unwrap();

        let blob = encode_linestring(&[(10.0_f64, 50.0_f64), (20.0, 60.0)], 4326);
        gpkg.conn
            .execute(
                "INSERT INTO flight_lines (line_index, geom) VALUES (0, ?1)",
                rusqlite::params![blob],
            )
            .unwrap();

        let count: i32 = gpkg
            .conn
            .query_row(
                "SELECT count(*) FROM rtree_flight_lines_geom",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "one R-tree entry should exist after the insert");
    }
}
