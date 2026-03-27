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

// KML/KMZ boundary polygon importer.
//
// Parses KML `<Polygon>` elements to extract survey area boundaries.
// Supports:
//   - Exterior rings (`<outerBoundaryIs><LinearRing><coordinates>`)
//   - Interior rings / holes (`<innerBoundaryIs><LinearRing><coordinates>`)
//   - .kmz files (ZIP archives containing doc.kml or *.kml)
//   - Multi-polygon files (returns Vec<Polygon>)
//
// KML coordinates are (longitude, latitude[, altitude]) — note the lon-first
// order per the OGC KML 2.2 specification.

use std::io::{Cursor, Read};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::IoError;
use crate::math::geometry::Polygon;

/// Parse a KML or KMZ file and return all `<Polygon>` elements as `Polygon` values.
///
/// For .kmz files, the ZIP archive is opened and the first `.kml` file found
/// inside is parsed. For .kml files, the XML is parsed directly.
pub fn parse_boundary(path: &Path) -> Result<Vec<Polygon>, IoError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let xml_bytes = match ext.as_str() {
        "kmz" => read_kml_from_kmz(path)?,
        _ => std::fs::read(path)?,
    };

    parse_kml_polygons(&xml_bytes)
}

/// Extract the root .kml file from a .kmz ZIP archive.
///
/// Prefers `doc.kml` (the KMZ convention for the root document).
/// Falls back to the first `.kml` entry found if `doc.kml` is absent.
fn read_kml_from_kmz(path: &Path) -> Result<Vec<u8>, IoError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| IoError::Serialization(format!("invalid KMZ archive: {e}")))?;

    /*
     * First pass: look for the conventional root document (doc.kml).
     * This avoids nondeterminism when the archive contains multiple KML files.
     */
    if let Ok(mut entry) = archive.by_name("doc.kml") {
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        return Ok(buf);
    }

    /*
     * Fallback: return the first .kml entry found, regardless of name.
     */
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| IoError::Serialization(format!("KMZ entry error: {e}")))?;
        let name = entry.name().to_lowercase();
        if name.ends_with(".kml") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }

    Err(IoError::Serialization(
        "KMZ archive contains no .kml file".into(),
    ))
}

/// Parse KML XML bytes and extract all `<Polygon>` elements.
fn parse_kml_polygons(xml: &[u8]) -> Result<Vec<Polygon>, IoError> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(true);

    let mut polygons = Vec::new();
    let mut buf = Vec::new();

    /*
     * State machine:
     *   - When we enter <Polygon>, start collecting rings.
     *   - <outerBoundaryIs><LinearRing><coordinates> → exterior ring
     *   - <innerBoundaryIs><LinearRing><coordinates> → interior ring
     *   - On </Polygon>, emit the polygon.
     */
    let mut in_polygon = false;
    let mut in_outer = false;
    let mut in_inner = false;
    let mut in_coordinates = false;
    let mut exterior: Vec<(f64, f64)> = Vec::new();
    let mut interiors: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current_ring: Vec<(f64, f64)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let local = local_name(&name_bytes);
                match local {
                    "Polygon" => {
                        in_polygon = true;
                        exterior.clear();
                        interiors.clear();
                    }
                    "outerBoundaryIs" if in_polygon => in_outer = true,
                    "innerBoundaryIs" if in_polygon => in_inner = true,
                    "coordinates" if in_polygon && (in_outer || in_inner) => {
                        in_coordinates = true;
                        current_ring.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let local = local_name(&name_bytes);
                match local {
                    "Polygon" => {
                        if !exterior.is_empty() {
                            polygons.push(Polygon::with_holes(
                                std::mem::take(&mut exterior),
                                std::mem::take(&mut interiors),
                            ));
                        }
                        in_polygon = false;
                    }
                    "outerBoundaryIs" => in_outer = false,
                    "innerBoundaryIs" => in_inner = false,
                    "coordinates" if in_coordinates => {
                        if in_outer {
                            exterior = std::mem::take(&mut current_ring);
                        } else if in_inner {
                            interiors.push(std::mem::take(&mut current_ring));
                        }
                        in_coordinates = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_coordinates => {
                let text = e.unescape()
                    .map_err(|err| IoError::Serialization(format!("KML text error: {err}")))?;
                parse_coordinate_text(&text, &mut current_ring)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(IoError::Serialization(format!(
                    "KML parse error at position {}: {e}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(polygons)
}

/// Parse KML coordinate text: "lon,lat[,alt] lon,lat[,alt] ..."
///
/// KML spec: coordinates are longitude,latitude[,altitude] separated by
/// whitespace. We store as (latitude, longitude) to match our internal
/// convention.
fn parse_coordinate_text(
    text: &str,
    ring: &mut Vec<(f64, f64)>,
) -> Result<(), IoError> {
    for tuple in text.split_whitespace() {
        let parts: Vec<&str> = tuple.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let lon: f64 = parts[0]
            .parse()
            .map_err(|_| IoError::Serialization(format!("invalid KML longitude: {}", parts[0])))?;
        let lat: f64 = parts[1]
            .parse()
            .map_err(|_| IoError::Serialization(format!("invalid KML latitude: {}", parts[1])))?;
        ring.push((lat, lon));
    }
    Ok(())
}

/// Extract the local name from a potentially namespace-prefixed tag.
/// e.g. "kml:Polygon" → "Polygon", "coordinates" → "coordinates".
fn local_name(name: &[u8]) -> &str {
    let s = std::str::from_utf8(name).unwrap_or("");
    s.rsplit(':').next().unwrap_or(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn simple_kml() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <Polygon>
        <outerBoundaryIs>
          <LinearRing>
            <coordinates>
              -0.1,51.5,0 0.0,51.5,0 0.0,51.6,0 -0.1,51.6,0 -0.1,51.5,0
            </coordinates>
          </LinearRing>
        </outerBoundaryIs>
      </Polygon>
    </Placemark>
  </Document>
</kml>"#
    }

    #[test]
    fn parse_simple_polygon() {
        let polygons = parse_kml_polygons(simple_kml().as_bytes()).unwrap();
        assert_eq!(polygons.len(), 1);
        /*
         * KML coordinates are lon,lat. We store as (lat, lon).
         * First vertex: lon=-0.1, lat=51.5 → (51.5, -0.1).
         */
        let ext = &polygons[0].exterior;
        assert!(ext.len() >= 4, "exterior ring should have >= 4 vertices");
        assert!((ext[0].0 - 51.5).abs() < 1e-9, "first lat");
        assert!((ext[0].1 - (-0.1)).abs() < 1e-9, "first lon");
    }

    #[test]
    fn parse_polygon_with_hole() {
        let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <Polygon>
      <outerBoundaryIs>
        <LinearRing>
          <coordinates>0,0 10,0 10,10 0,10 0,0</coordinates>
        </LinearRing>
      </outerBoundaryIs>
      <innerBoundaryIs>
        <LinearRing>
          <coordinates>3,3 7,3 7,7 3,7 3,3</coordinates>
        </LinearRing>
      </innerBoundaryIs>
    </Polygon>
  </Placemark>
</kml>"#;
        let polygons = parse_kml_polygons(kml.as_bytes()).unwrap();
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].interiors.len(), 1);
        assert!(polygons[0].contains(1.0, 1.0), "between outer and hole");
        assert!(!polygons[0].contains(5.0, 5.0), "inside hole");
    }

    #[test]
    fn parse_multi_polygon() {
        let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <Polygon>
        <outerBoundaryIs>
          <LinearRing><coordinates>0,0 1,0 1,1 0,1 0,0</coordinates></LinearRing>
        </outerBoundaryIs>
      </Polygon>
    </Placemark>
    <Placemark>
      <Polygon>
        <outerBoundaryIs>
          <LinearRing><coordinates>5,5 6,5 6,6 5,6 5,5</coordinates></LinearRing>
        </outerBoundaryIs>
      </Polygon>
    </Placemark>
  </Document>
</kml>"#;
        let polygons = parse_kml_polygons(kml.as_bytes()).unwrap();
        assert_eq!(polygons.len(), 2);
    }

    #[test]
    fn parse_kmz_file() {
        let dir = tempdir().unwrap();
        let kmz_path = dir.path().join("boundary.kmz");

        /*
         * Create a minimal KMZ (ZIP containing a .kml file).
         */
        let kml_content = simple_kml();
        let file = std::fs::File::create(&kmz_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("doc.kml", options).unwrap();
        std::io::Write::write_all(&mut zip, kml_content.as_bytes()).unwrap();
        zip.finish().unwrap();

        let polygons = parse_boundary(&kmz_path).unwrap();
        assert_eq!(polygons.len(), 1);
        assert!(polygons[0].exterior.len() >= 4);
    }

    #[test]
    fn parse_boundary_kml_file() {
        let dir = tempdir().unwrap();
        let kml_path = dir.path().join("boundary.kml");
        std::fs::write(&kml_path, simple_kml()).unwrap();

        let polygons = parse_boundary(&kml_path).unwrap();
        assert_eq!(polygons.len(), 1);
    }

    #[test]
    fn coordinate_order_is_lat_lon_internally() {
        /*
         * KML input: lon,lat. Internal storage: (lat, lon).
         * Verify the swap is correct.
         */
        let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <Polygon>
      <outerBoundaryIs>
        <LinearRing>
          <coordinates>-0.1,51.5 0.0,51.5 0.0,51.6 -0.1,51.6 -0.1,51.5</coordinates>
        </LinearRing>
      </outerBoundaryIs>
    </Polygon>
  </Placemark>
</kml>"#;
        let polys = parse_kml_polygons(kml.as_bytes()).unwrap();
        let (min_lat, min_lon, max_lat, max_lon) = polys[0].bbox();
        assert!(min_lat > 51.0 && min_lat < 52.0, "lat in expected range");
        assert!(min_lon > -1.0 && min_lon < 1.0, "lon in expected range");
        assert!(max_lat > min_lat);
        assert!(max_lon > min_lon);
    }
}
