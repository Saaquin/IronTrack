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

/// Create the R-tree virtual table and all 7 GeoPackage synchronization
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

    // All 7 sync triggers per GeoPackage spec Annex F.3
    conn.execute_batch(&build_trigger_sql(table, geom_col, pk_col))?;

    Ok(())
}

/// Produce the SQL for all 7 GeoPackage R-tree synchronization triggers.
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

-- 7. DELETE: row removed with a valid geometry
CREATE TRIGGER IF NOT EXISTS rtree_{t}_{c}_delete AFTER DELETE ON {t}
WHEN OLD.{c} NOT NULL AND NOT ST_IsEmpty(OLD.{c})
BEGIN
  DELETE FROM rtree_{t}_{c} WHERE id = OLD.{i};
END;
"#
    )
}
