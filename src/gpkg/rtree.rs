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

// R-tree spatial index setup for GeoPackage feature tables.
//
// Registers five custom SQLite scalar functions (ST_MinX, ST_MaxX, ST_MinY,
// ST_MaxY, ST_IsEmpty) that parse the StandardGeoPackageBinary envelope
// directly from the BLOB bytes. This avoids a SpatiaLite dependency while
// still satisfying the trigger logic required by the GeoPackage spec.
//
// The scalar functions are session-level — they must be re-registered on
// every `GeoPackage::open()` and `GeoPackage::new()` call before any trigger
// that references them can fire.

use crate::error::GpkgError;
use rusqlite::{functions::FunctionFlags, Connection};

use super::binary::{
    parse_envelope_maxx, parse_envelope_maxy, parse_envelope_minx, parse_envelope_miny,
    parse_is_empty,
};

/// Register ST_MinX, ST_MaxX, ST_MinY, ST_MaxY, ST_IsEmpty as session-level
/// SQLite scalar functions on `conn`.
///
/// These functions parse the 2D envelope baked into the StandardGeoPackageBinary
/// header at known byte offsets (bytes 8..40). Reading four f64 values from a
/// fixed offset is dramatically cheaper than decoding full WKB geometry.
pub fn register_st_functions(conn: &Connection) -> Result<(), GpkgError> {
    let det = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;

    conn.create_scalar_function("ST_MinX", 1, det, |ctx| {
        let blob: Vec<u8> = ctx.get(0)?;
        Ok(parse_envelope_minx(&blob))
    })?;

    conn.create_scalar_function("ST_MaxX", 1, det, |ctx| {
        let blob: Vec<u8> = ctx.get(0)?;
        Ok(parse_envelope_maxx(&blob))
    })?;

    conn.create_scalar_function("ST_MinY", 1, det, |ctx| {
        let blob: Vec<u8> = ctx.get(0)?;
        Ok(parse_envelope_miny(&blob))
    })?;

    conn.create_scalar_function("ST_MaxY", 1, det, |ctx| {
        let blob: Vec<u8> = ctx.get(0)?;
        Ok(parse_envelope_maxy(&blob))
    })?;

    /*
     * ST_IsEmpty returns 1 for empty geometries, 0 otherwise.
     * Triggers use `NOT ST_IsEmpty(...)` to guard insert/update paths.
     * We return i32 rather than bool because SQLite has no native boolean type.
     */
    conn.create_scalar_function("ST_IsEmpty", 1, det, |ctx| {
        let blob: Vec<u8> = ctx.get(0)?;
        Ok(parse_is_empty(&blob) as i32)
    })?;

    Ok(())
}

/// Create the R-tree virtual table and all 8 GeoPackage synchronization
/// triggers for a feature table.
///
/// Arguments:
/// - `conn`: open rusqlite connection with ST_ functions already registered
/// - `table`: feature table name (must be ASCII alphanumeric + underscores)
/// - `geom_col`: geometry column name (typically "geom")
/// - `pk_col`: primary key column name (typically "id")
///
/// Also inserts the extension registration row into `gpkg_extensions`.
pub fn create_rtree_index(
    conn: &Connection,
    table: &str,
    geom_col: &str,
    pk_col: &str,
) -> Result<(), GpkgError> {
    /*
     * Register the R-tree extension in gpkg_extensions so compliant GIS
     * software knows to use the spatial index when opening this file.
     */
    conn.execute(
        "INSERT OR IGNORE INTO gpkg_extensions \
         (table_name, column_name, extension_name, definition, scope) \
         VALUES (?1, ?2, 'gpkg_rtree_index', \
                 'http://www.geopackage.org/spec/#extension_rtree', \
                 'write-only')",
        rusqlite::params![table, geom_col],
    )?;

    // Create the R*Tree virtual table
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS rtree_{table}_{geom_col} \
         USING rtree(id, minx, maxx, miny, maxy);"
    ))?;

    // All 8 sync triggers per GeoPackage spec Annex F.3
    conn.execute_batch(&build_trigger_sql(table, geom_col, pk_col))?;

    Ok(())
}

/// Produce the SQL for all 8 GeoPackage R-tree synchronization triggers.
/// Parameters follow the spec naming convention: <t> table, <c> geom column, <i> PK.
fn build_trigger_sql(t: &str, c: &str, i: &str) -> String {
    format!(
        r#"
-- 1. INSERT: new row with non-null, non-empty geometry
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_insert AFTER INSERT ON {t}
WHEN NEW.{c} NOT NULL AND NOT ST_IsEmpty(NEW.{c})
BEGIN
  INSERT INTO rtree_{t}_{c} VALUES (
    NEW.{i},
    ST_MinX(NEW.{c}), ST_MaxX(NEW.{c}),
    ST_MinY(NEW.{c}), ST_MaxY(NEW.{c}));
END;

-- 2. UPDATE1: geometry changed, PK unchanged, both non-null and non-empty
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update1 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} = NEW.{i}
  AND NEW.{c} NOT NULL AND NOT ST_IsEmpty(NEW.{c})
  AND OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
BEGIN
  UPDATE rtree_{t}_{c} SET
    minx = ST_MinX(NEW.{c}), maxx = ST_MaxX(NEW.{c}),
    miny = ST_MinY(NEW.{c}), maxy = ST_MaxY(NEW.{c})
  WHERE id = NEW.{i};
END;

-- 3. UPDATE2: PK changed, both geometries non-null and non-empty
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update2 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} != NEW.{i}
  AND NEW.{c} NOT NULL AND NOT ST_IsEmpty(NEW.{c})
  AND OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
BEGIN
  DELETE FROM rtree_{t}_{c} WHERE id = OLD.{i};
  INSERT INTO rtree_{t}_{c} VALUES (
    NEW.{i},
    ST_MinX(NEW.{c}), ST_MaxX(NEW.{c}),
    ST_MinY(NEW.{c}), ST_MaxY(NEW.{c}));
END;

-- 4. UPDATE3: PK changed, new geometry non-null/non-empty, old was null/empty
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update3 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} != NEW.{i}
  AND NEW.{c} NOT NULL AND NOT ST_IsEmpty(NEW.{c})
  AND (OLD.{c} IS NULL OR ST_IsEmpty(OLD.{c}))
BEGIN
  INSERT INTO rtree_{t}_{c} VALUES (
    NEW.{i},
    ST_MinX(NEW.{c}), ST_MaxX(NEW.{c}),
    ST_MinY(NEW.{c}), ST_MaxY(NEW.{c}));
END;

-- 5. UPDATE4: PK changed, old geometry non-null/non-empty, new is null/empty
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update4 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} != NEW.{i}
  AND OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
  AND (NEW.{c} IS NULL OR ST_IsEmpty(NEW.{c}))
BEGIN
  DELETE FROM rtree_{t}_{c} WHERE id = OLD.{i};
END;

-- 6. UPDATE5 (spec): PK unchanged, old was null/empty, new geometry is valid
--    This is the "fill in geometry later" pattern. Without this trigger the
--    R-tree never gets an entry for rows whose geometry column is populated
--    after the initial insert, producing silently incorrect spatial queries.
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update5 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} = NEW.{i}
  AND NEW.{c} NOT NULL AND NOT ST_IsEmpty(NEW.{c})
  AND (OLD.{c} IS NULL OR ST_IsEmpty(OLD.{c}))
BEGIN
  INSERT INTO rtree_{t}_{c} VALUES (
    NEW.{i},
    ST_MinX(NEW.{c}), ST_MaxX(NEW.{c}),
    ST_MinY(NEW.{c}), ST_MaxY(NEW.{c}));
END;

-- 7. UPDATE6 (spec): PK unchanged, geometry set to null or empty, old was valid
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_update6 AFTER UPDATE OF {c} ON {t}
WHEN OLD.{i} = NEW.{i}
  AND (NEW.{c} IS NULL OR ST_IsEmpty(NEW.{c}))
  AND OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
BEGIN
  DELETE FROM rtree_{t}_{c} WHERE id = OLD.{i};
END;

-- 8. DELETE: row removed with a valid geometry
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_delete AFTER DELETE ON {t}
WHEN OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
BEGIN
  DELETE FROM rtree_{t}_{c} WHERE id = OLD.{i};
END;
"#
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::gpkg::binary::encode_linestring;
    use crate::gpkg::init::GeoPackage;
    use tempfile::tempdir;

    const TABLE: &str = "test_features";

    /// Create a temp GeoPackage with a feature table and R-tree index.
    fn setup() -> (tempfile::TempDir, GeoPackage) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rtree_test.gpkg");
        let gpkg = GeoPackage::new(&path).unwrap();
        gpkg.create_feature_table(TABLE).unwrap();
        (dir, gpkg)
    }

    /// Insert a 2-point linestring and return its auto-incremented row id.
    fn insert_line(gpkg: &GeoPackage, line_index: i32, coords: &[(f64, f64)]) -> i64 {
        let blob = encode_linestring(coords, 4326);
        gpkg.conn
            .execute(
                "INSERT INTO test_features (line_index, geom) VALUES (?1, ?2)",
                rusqlite::params![line_index, blob],
            )
            .unwrap();
        gpkg.conn.last_insert_rowid()
    }

    /// Query R-tree for entries whose bbox contains the given point.
    fn query_point(gpkg: &GeoPackage, x: f64, y: f64) -> Vec<i64> {
        let mut stmt = gpkg
            .conn
            .prepare(
                "SELECT id FROM rtree_test_features_geom \
                 WHERE minx <= ?1 AND maxx >= ?1 AND miny <= ?2 AND maxy >= ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![x, y], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Query R-tree for entries whose bbox intersects the given bbox.
    fn query_bbox(gpkg: &GeoPackage, minx: f64, maxx: f64, miny: f64, maxy: f64) -> Vec<i64> {
        let mut stmt = gpkg
            .conn
            .prepare(
                "SELECT id FROM rtree_test_features_geom \
                 WHERE maxx >= ?1 AND minx <= ?2 AND maxy >= ?3 AND miny <= ?4",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![minx, maxx, miny, maxy], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    // (a) Insert a single feature, query its bounding box, verify it's returned.
    #[test]
    fn single_feature_found_by_bbox_query() {
        let (_dir, gpkg) = setup();
        let id = insert_line(&gpkg, 0, &[(10.0, 50.0), (20.0, 60.0)]);

        let results = query_point(&gpkg, 15.0, 55.0);
        assert_eq!(results, vec![id]);
    }

    // (b) Insert 100 features with known bounding boxes, verify point-in-bbox
    //     queries return the correct subset.
    #[test]
    fn hundred_features_point_query_returns_correct_subset() {
        let (_dir, gpkg) = setup();

        // 10×10 grid: each feature is a 1°×1° box on a 2° spacing grid.
        // Feature at (row, col) has bbox x=[col*2, col*2+1], y=[row*2, row*2+1].
        let mut ids = Vec::with_capacity(100);
        for row in 0..10_i32 {
            for col in 0..10_i32 {
                let x0 = col as f64 * 2.0;
                let y0 = row as f64 * 2.0;
                let id = insert_line(&gpkg, row * 10 + col, &[(x0, y0), (x0 + 1.0, y0 + 1.0)]);
                ids.push(id);
            }
        }

        // Point (4.5, 4.5) → inside grid cell (row=2, col=2): bbox [4,5]×[4,5]
        let results = query_point(&gpkg, 4.5, 4.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ids[2 * 10 + 2]);

        // Point (0.5, 0.5) → inside grid cell (0, 0): bbox [0,1]×[0,1]
        let results = query_point(&gpkg, 0.5, 0.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ids[0]);

        // Point (18.5, 18.5) → inside grid cell (9, 9): bbox [18,19]×[18,19]
        let results = query_point(&gpkg, 18.5, 18.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ids[9 * 10 + 9]);
    }

    // (c) Query outside all bounding boxes → empty result.
    #[test]
    fn query_outside_all_bboxes_returns_empty() {
        let (_dir, gpkg) = setup();
        for i in 0..5_i32 {
            let x = i as f64;
            insert_line(&gpkg, i, &[(x, x), (x + 1.0, x + 1.0)]);
        }
        // All features within [0,6]×[0,6]. Query far outside.
        assert!(query_point(&gpkg, 100.0, 100.0).is_empty());
        assert!(query_point(&gpkg, -50.0, -50.0).is_empty());
    }

    // (d) Overlapping bounding boxes → all overlapping features returned.
    #[test]
    fn overlapping_bboxes_all_returned() {
        let (_dir, gpkg) = setup();
        let id1 = insert_line(&gpkg, 0, &[(8.0, 48.0), (12.0, 52.0)]);
        let id2 = insert_line(&gpkg, 1, &[(9.0, 49.0), (11.0, 51.0)]);
        let id3 = insert_line(&gpkg, 2, &[(7.0, 47.0), (13.0, 53.0)]);

        // (10.0, 50.0) is inside all three bboxes.
        let mut results = query_point(&gpkg, 10.0, 50.0);
        results.sort();
        let mut expected = vec![id1, id2, id3];
        expected.sort();
        assert_eq!(results, expected);

        // (8.5, 48.5) is inside id1 and id3 but NOT id2 (id2 miny=49).
        let mut results = query_point(&gpkg, 8.5, 48.5);
        results.sort();
        let mut expected = vec![id1, id3];
        expected.sort();
        assert_eq!(results, expected);
    }

    // (e) Feature at the antimeridian (lon ~±180°).
    #[test]
    fn feature_at_antimeridian() {
        let (_dir, gpkg) = setup();

        // Positive side: lon 179° to 180°
        let id_pos = insert_line(&gpkg, 0, &[(179.0, 0.0), (180.0, 1.0)]);
        let results = query_point(&gpkg, 179.5, 0.5);
        assert_eq!(results, vec![id_pos]);

        // Negative side: lon -180° to -179°
        let id_neg = insert_line(&gpkg, 1, &[(-180.0, -1.0), (-179.0, 0.0)]);
        let results = query_point(&gpkg, -179.5, -0.5);
        assert_eq!(results, vec![id_neg]);

        // Bbox query spanning ±180° should find both features.
        let mut results = query_bbox(&gpkg, 179.0, 180.0, -1.0, 1.0);
        results.sort();
        assert!(results.contains(&id_pos));

        let mut results = query_bbox(&gpkg, -180.0, -179.0, -1.0, 0.0);
        results.sort();
        assert!(results.contains(&id_neg));
    }

    // (f) Verify the R-tree trigger SQL is syntactically valid and executes
    //     without error on a fresh GeoPackage.
    #[test]
    fn trigger_sql_valid_on_fresh_gpkg() {
        let (_dir, gpkg) = setup();

        // Count all triggers created for the test_features table.
        // GeoPackage spec: insert + 6 update variants + delete = 8 triggers.
        let trigger_count: i64 = gpkg
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master \
                 WHERE type = 'trigger' AND name LIKE 'rtree_test_features_geom%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            trigger_count, 8,
            "GeoPackage spec requires 8 R-tree sync triggers (insert + 6 update + delete)"
        );

        // Verify the R-tree virtual table exists.
        let rtree_exists: bool = gpkg
            .conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master \
                 WHERE type = 'table' AND name = 'rtree_test_features_geom'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(rtree_exists, "R-tree virtual table must exist");

        // Verify the extension is registered in gpkg_extensions.
        let ext_count: i64 = gpkg
            .conn
            .query_row(
                "SELECT count(*) FROM gpkg_extensions \
                 WHERE table_name = 'test_features' \
                   AND extension_name = 'gpkg_rtree_index'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ext_count, 1, "R-tree extension must be registered");
    }

    // Regression: an R-tree query on an empty table should not error.
    #[test]
    fn empty_table_bbox_query_returns_empty() {
        let (_dir, gpkg) = setup();
        assert!(query_point(&gpkg, 0.0, 0.0).is_empty());
        assert!(query_bbox(&gpkg, -180.0, 180.0, -90.0, 90.0).is_empty());
    }
}
