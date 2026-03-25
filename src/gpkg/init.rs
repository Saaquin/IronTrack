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

// GeoPackage database initialization, mandatory table creation, and the
// high-level FlightPlan insertion API.
//
// GeoPackage spec §1.1: "A GeoPackage is a platform-independent SQLite
// database file containing data and metadata tables with specified names,
// integrity constraints, formats, and contents."
//
// Two-Phase I/O note: `insert_flight_plan` performs only sequential SQLite
// writes — there is no blocking DEM/network I/O here, so no Rayon parallelism
// is needed or appropriate.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::GpkgError;
use crate::photogrammetry::FlightPlan;

use super::binary;
use super::rtree;

/// WKT definition for EPSG:4326 seeded into gpkg_spatial_ref_sys.
/// Uses the WKT1 authority string accepted by GDAL and QGIS.
const WGS84_WKT: &str = concat!(
    r#"GEOGCS["WGS 84","#,
    r#"DATUM["WGS_1984","#,
    r#"SPHEROID["WGS 84",6378137,298.257223563,"#,
    r#"AUTHORITY["EPSG","7030"]]],"#,
    r#"PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],"#,
    r#"UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],"#,
    r#"AUTHORITY["EPSG","4326"]]"#
);

/// A GeoPackage database handle.
///
/// Wraps a `rusqlite::Connection` with the GeoPackage PRAGMAs applied and
/// all custom ST_ scalar functions registered. Obtain via `new` or `open`.
pub struct GeoPackage {
    /// Public within the crate so `binary.rs` / `rtree.rs` integration tests
    /// can query internal state directly without exposing it to callers.
    pub(crate) conn: Connection,
}

impl GeoPackage {
    /// Create a new GeoPackage at `path`.
    ///
    /// Applies all PRAGMAs, creates the four mandatory tables, and seeds
    /// `gpkg_spatial_ref_sys` with the required WGS84, undefined-Cartesian,
    /// and undefined-geographic records.
    ///
    /// Returns `GpkgError::Init` if the path already exists as a non-SQLite
    /// file; rusqlite will propagate a `Database` error in that case anyway.
    pub fn new(path: &Path) -> Result<Self, GpkgError> {
        let conn = Connection::open(path)?;
        /*
         * page_size MUST be set before the first table is written.
         * SQLite ignores this pragma once any data page has been allocated,
         * so it is safe to attempt on an existing file — it just has no effect.
         */
        conn.execute_batch("PRAGMA page_size = 4096;")?;
        let gpkg = Self { conn };
        gpkg.apply_creation_pragmas()?;
        gpkg.apply_session_pragmas()?;
        rtree::register_st_functions(&gpkg.conn)?;
        gpkg.create_mandatory_tables()?;
        gpkg.seed_spatial_ref_sys()?;
        Ok(gpkg)
    }

    /// Open an existing GeoPackage at `path`.
    ///
    /// Validates that the file is a GeoPackage by checking `PRAGMA
    /// application_id` (must be 0x47504B47 = 1196444487). Returns
    /// `GpkgError::Init` if the file is not a valid GeoPackage.
    /// Applies session-level PRAGMAs and re-registers ST_ scalar functions.
    pub fn open(path: &Path) -> Result<Self, GpkgError> {
        let conn = Connection::open(path)?;
        let gpkg = Self { conn };
        gpkg.apply_session_pragmas()?;
        /*
         * Validate that the file was initialised as a GeoPackage before doing
         * anything else. An application_id of 0 means a plain SQLite file.
         * Catching this here prevents opaque late failures from missing tables.
         */
        let app_id: i64 = gpkg
            .conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))?;
        if app_id != 1_196_444_487 {
            return Err(GpkgError::Init(format!(
                "file is not a GeoPackage (application_id={app_id:#010x}, expected 0x47504b47)"
            )));
        }
        rtree::register_st_functions(&gpkg.conn)?;
        Ok(gpkg)
    }

    // -----------------------------------------------------------------------
    // Pragma helpers
    // -----------------------------------------------------------------------

    /// Persistent PRAGMAs — written to the database file header.
    /// Only meaningful on database creation; harmless to re-apply.
    fn apply_creation_pragmas(&self) -> Result<(), GpkgError> {
        self.conn.execute_batch(
            "PRAGMA application_id = 1196444487;\
             PRAGMA user_version = 10300;",
        )?;
        Ok(())
    }

    /// Session-level PRAGMAs — reset to defaults when the connection closes.
    fn apply_session_pragmas(&self) -> Result<(), GpkgError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;\
             PRAGMA synchronous = NORMAL;\
             PRAGMA cache_size = -64000;\
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Schema setup
    // -----------------------------------------------------------------------

    /// Create the four mandatory GeoPackage metadata tables.
    ///
    /// All DDL uses IF NOT EXISTS so this is idempotent — safe to call on
    /// both new files and on re-opened existing ones.
    fn create_mandatory_tables(&self) -> Result<(), GpkgError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS gpkg_spatial_ref_sys (
                srs_name                 TEXT    NOT NULL,
                srs_id                   INTEGER NOT NULL PRIMARY KEY,
                organization             TEXT    NOT NULL,
                organization_coordsys_id INTEGER NOT NULL,
                definition               TEXT    NOT NULL,
                description              TEXT
            );

            CREATE TABLE IF NOT EXISTS gpkg_contents (
                table_name  TEXT     NOT NULL PRIMARY KEY,
                data_type   TEXT     NOT NULL,
                identifier  TEXT,
                description TEXT     DEFAULT '',
                last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                min_x       REAL,
                min_y       REAL,
                max_x       REAL,
                max_y       REAL,
                srs_id      INTEGER,
                CONSTRAINT fk_gc_r_srs_id
                    FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
            );

            CREATE TABLE IF NOT EXISTS gpkg_geometry_columns (
                table_name         TEXT    NOT NULL,
                column_name        TEXT    NOT NULL,
                geometry_type_name TEXT    NOT NULL,
                srs_id             INTEGER NOT NULL,
                z                  TINYINT NOT NULL,
                m                  TINYINT NOT NULL,
                CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
                CONSTRAINT fk_gc_tn
                    FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
                CONSTRAINT fk_gc_srs
                    FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id)
            );

            CREATE TABLE IF NOT EXISTS gpkg_extensions (
                table_name     TEXT,
                column_name    TEXT,
                extension_name TEXT NOT NULL,
                definition     TEXT NOT NULL,
                scope          TEXT NOT NULL,
                CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name)
            );
            "#,
        )?;
        Ok(())
    }

    /// Insert the three required seed rows into gpkg_spatial_ref_sys.
    fn seed_spatial_ref_sys(&self) -> Result<(), GpkgError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys \
             (srs_name, srs_id, organization, organization_coordsys_id, definition, description) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "Undefined Cartesian Coordinate Reference System",
                -1_i32,
                "NONE",
                -1_i32,
                "undefined",
                "undefined Cartesian coordinate reference system"
            ],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys \
             (srs_name, srs_id, organization, organization_coordsys_id, definition, description) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "Undefined Geographic Coordinate Reference System",
                0_i32,
                "NONE",
                0_i32,
                "undefined",
                "undefined geographic coordinate reference system"
            ],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO gpkg_spatial_ref_sys \
             (srs_name, srs_id, organization, organization_coordsys_id, definition, description) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "WGS 84 geodetic",
                4326_i32,
                "EPSG",
                4326_i32,
                WGS84_WKT,
                "longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid"
            ],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Feature table management
    // -----------------------------------------------------------------------

    /// Create a user feature table for `LINESTRINGZ` geometries and register
    /// it in the mandatory GeoPackage metadata tables. Installs an R-tree
    /// spatial index automatically.
    ///
    /// `table_name` must contain only ASCII alphanumeric characters and
    /// underscores to prevent SQL injection through the table name.
    pub fn create_feature_table(&self, table_name: &str) -> Result<(), GpkgError> {
        validate_identifier(table_name)?;

        self.conn.execute_batch(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {table_name} (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                line_index INTEGER NOT NULL,
                geom       BLOB    NOT NULL
            );
            "#
        ))?;

        self.conn.execute(
            "INSERT OR IGNORE INTO gpkg_contents \
             (table_name, data_type, identifier, srs_id) \
             VALUES (?1, 'features', ?1, 4326)",
            params![table_name],
        )?;

        self.conn.execute(
            "INSERT OR IGNORE INTO gpkg_geometry_columns \
             (table_name, column_name, geometry_type_name, srs_id, z, m) \
             VALUES (?1, 'geom', 'LINESTRINGZ', 4326, 1, 0)",
            params![table_name],
        )?;

        rtree::create_rtree_index(&self.conn, table_name, "geom", "id")?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Flight plan I/O
    // -----------------------------------------------------------------------

    /// Insert all flight lines from `plan` into `table_name`.
    ///
    /// Each line is serialized to a StandardGeoPackageBinary blob via
    /// `FlightLine::iter_lonlat_deg()` and stored with its zero-based index.
    /// After insertion, the bounding box in `gpkg_contents` is updated to
    /// encompass the full plan extent.
    ///
    /// All writes are wrapped in a single transaction. If any step fails, the
    /// database is rolled back to its pre-call state — no partial rows remain.
    ///
    /// The table must have been created with `create_feature_table` first.
    pub fn insert_flight_plan(
        &self,
        table_name: &str,
        plan: &FlightPlan,
    ) -> Result<(), GpkgError> {
        validate_identifier(table_name)?;

        /*
         * Explicit transaction: the iterator may span hundreds of lines.
         * Without a transaction every INSERT is auto-committed (one fsync each),
         * which is ~100× slower and leaves the table in a partial state on error.
         */
        self.conn.execute_batch("BEGIN;")?;

        let result = (|| {
            let mut stmt = self.conn.prepare(&format!(
                "INSERT INTO {table_name} (line_index, geom) VALUES (?1, ?2)"
            ))?;

            for (idx, line) in plan.lines.iter().enumerate() {
                /*
                 * Collect (lon, lat, elevation_msl) triples so the blob
                 * preserves altitude data as a WKB LineStringZ (type 1002).
                 * The elevation comes from the terrain-adjusted waypoint
                 * stored in FlightLine, or the flat-terrain MSL value when
                 * no DEM is used.
                 */
                let coords: Vec<(f64, f64, f64)> = line
                    .iter_lonlat_deg()
                    .zip(line.elevations())
                    .map(|((lon, lat), &elev)| (lon, lat, elev))
                    .collect();
                let blob = binary::encode_linestring_z(&coords, 4326);
                stmt.execute(params![idx as i64, blob])?;
            }

            /*
             * Update the gpkg_contents bounding box to the extent of the full
             * plan. GIS viewers use this for initial zoom-to-layer.
             */
            if let Some((min_x, min_y, max_x, max_y)) = plan_bbox(plan) {
                self.conn.execute(
                    "UPDATE gpkg_contents SET min_x=?1, min_y=?2, max_x=?3, max_y=?4 \
                     WHERE table_name=?5",
                    params![min_x, min_y, max_x, max_y, table_name],
                )?;
            }

            Ok::<(), GpkgError>(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(e) => {
                // Best-effort rollback; ignore secondary error so the original is returned.
                let _ = self.conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Count the rows in `table_name`.
    ///
    /// Primarily used in integration tests to assert that `insert_flight_plan`
    /// wrote the expected number of rows without requiring direct access to
    /// the internal `Connection`.
    pub fn count_rows(&self, table_name: &str) -> Result<i64, GpkgError> {
        validate_identifier(table_name)?;
        let count: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM {table_name}"),
            [],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Count the entries in the R-tree shadow table for a feature table.
    ///
    /// The R-tree virtual table is named `rtree_<table_name>_<geom_col>`.
    /// Used in integration tests to verify all 7 sync triggers are firing.
    pub fn count_rtree_entries(&self, table_name: &str) -> Result<i64, GpkgError> {
        validate_identifier(table_name)?;
        let count: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM rtree_{table_name}_geom"),
            [],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Return the bounding box stored in `gpkg_contents` for `table_name`.
    ///
    /// Returns `(min_x, min_y, max_x, max_y)` as `Option<f64>` values — all
    /// four are None until `insert_flight_plan` has been called at least once.
    #[allow(clippy::type_complexity)]
    pub fn contents_bbox(
        &self,
        table_name: &str,
    ) -> Result<(Option<f64>, Option<f64>, Option<f64>, Option<f64>), GpkgError> {
        validate_identifier(table_name)?;
        let bbox = self.conn.query_row(
            &format!(
                "SELECT min_x, min_y, max_x, max_y FROM gpkg_contents \
                 WHERE table_name='{table_name}'"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(bbox)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute the (min_lon, min_lat, max_lon, max_lat) bounding box of a plan.
/// Returns None if the plan has no waypoints.
fn plan_bbox(plan: &FlightPlan) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut found = false;
    for line in &plan.lines {
        for (lon, lat) in line.iter_lonlat_deg() {
            found = true;
            if lon < min_x {
                min_x = lon;
            }
            if lon > max_x {
                max_x = lon;
            }
            if lat < min_y {
                min_y = lat;
            }
            if lat > max_y {
                max_y = lat;
            }
        }
    }
    if found {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Reject identifiers that are not purely ASCII alphanumeric + underscores.
/// This prevents format-string SQL injection through table/column names.
fn validate_identifier(name: &str) -> Result<(), GpkgError> {
    if name.is_empty() {
        return Err(GpkgError::Init("identifier must not be empty".into()));
    }
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        Ok(())
    } else {
        Err(GpkgError::Init(format!(
            "invalid identifier {name:?}: only ASCII alphanumeric and underscores are allowed"
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tmp_gpkg() -> (tempfile::TempDir, GeoPackage) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.gpkg");
        let gpkg = GeoPackage::new(&path).unwrap();
        (dir, gpkg)
    }

    // --- PRAGMA checks ---

    #[test]
    fn new_gpkg_has_wal_mode() {
        let (_dir, gpkg) = tmp_gpkg();
        let mode: String = gpkg
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn application_id_is_gpkg_magic() {
        let (_dir, gpkg) = tmp_gpkg();
        let id: i64 = gpkg
            .conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .unwrap();
        // 0x47504B47 = 1196444487
        assert_eq!(id, 1_196_444_487);
    }

    #[test]
    fn user_version_is_gpkg_1_3_0() {
        let (_dir, gpkg) = tmp_gpkg();
        let v: i64 = gpkg
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 10300);
    }

    // --- Mandatory tables ---

    #[test]
    fn all_mandatory_tables_exist() {
        let (_dir, gpkg) = tmp_gpkg();
        for table in &[
            "gpkg_spatial_ref_sys",
            "gpkg_contents",
            "gpkg_geometry_columns",
            "gpkg_extensions",
        ] {
            let count: i32 = gpkg
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master \
                     WHERE type='table' AND name=?1",
                    params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "mandatory table {table} must exist");
        }
    }

    // --- Seed data ---

    #[test]
    fn spatial_ref_sys_seeded_with_required_records() {
        let (_dir, gpkg) = tmp_gpkg();
        for srs_id in &[-1_i32, 0, 4326] {
            let count: i32 = gpkg
                .conn
                .query_row(
                    "SELECT count(*) FROM gpkg_spatial_ref_sys WHERE srs_id=?1",
                    params![srs_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "srs_id {srs_id} must be seeded");
        }
    }

    #[test]
    fn wgs84_wkt_contains_authority_epsg_4326() {
        let (_dir, gpkg) = tmp_gpkg();
        let wkt: String = gpkg
            .conn
            .query_row(
                "SELECT definition FROM gpkg_spatial_ref_sys WHERE srs_id=4326",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            wkt.contains("EPSG") && wkt.contains("4326"),
            "WKT must contain EPSG:4326 authority: {wkt}"
        );
    }

    // --- Round-trip: open an existing file ---

    #[test]
    fn open_reads_existing_gpkg() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("round_trip.gpkg");
        // Create and close
        { GeoPackage::new(&path).unwrap(); }
        // Re-open and verify the mandatory tables survived
        let gpkg = GeoPackage::open(&path).unwrap();
        let count: i32 = gpkg
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' \
                 AND name='gpkg_spatial_ref_sys'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // --- Feature table management ---

    #[test]
    fn create_feature_table_registers_in_gpkg_contents() {
        let (_dir, gpkg) = tmp_gpkg();
        gpkg.create_feature_table("flight_lines").unwrap();

        let data_type: String = gpkg
            .conn
            .query_row(
                "SELECT data_type FROM gpkg_contents WHERE table_name='flight_lines'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(data_type, "features");
    }

    #[test]
    fn create_feature_table_registers_geometry_columns() {
        let (_dir, gpkg) = tmp_gpkg();
        gpkg.create_feature_table("flight_lines").unwrap();

        let geom_type: String = gpkg
            .conn
            .query_row(
                "SELECT geometry_type_name FROM gpkg_geometry_columns \
                 WHERE table_name='flight_lines'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(geom_type, "LINESTRINGZ");
    }

    #[test]
    fn create_feature_table_invalid_name_is_rejected() {
        let (_dir, gpkg) = tmp_gpkg();
        let result = gpkg.create_feature_table("bad name; DROP TABLE gpkg_contents;--");
        assert!(result.is_err(), "invalid table name must be rejected");
    }

    // --- Identifier validation ---

    #[test]
    fn validate_identifier_accepts_alphanumeric_underscore() {
        assert!(validate_identifier("flight_lines_2024").is_ok());
        assert!(validate_identifier("FlightLines").is_ok());
    }

    #[test]
    fn validate_identifier_rejects_spaces_and_punctuation() {
        assert!(validate_identifier("bad name").is_err());
        assert!(validate_identifier("bad-name").is_err());
        assert!(validate_identifier("bad;name").is_err());
    }

    #[test]
    fn validate_identifier_rejects_empty_string() {
        assert!(validate_identifier("").is_err());
    }
}
