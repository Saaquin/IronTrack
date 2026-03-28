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

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use irontrack::ai::{ClaudeClient, NlPlanParams};
use irontrack::dem::DemType;
use irontrack::dem::ElevationSource::{CopernicusDem, OceanFallback, SeaLevelFallback};
use irontrack::dem::TerrainEngine;
use irontrack::error::DemError;
use irontrack::gpkg::GeoPackage;
use irontrack::io::{
    parse_boundary, parse_centerline, write_dji_kmz, write_geojson, write_qgc_plan,
};
use irontrack::legal;
use irontrack::math::dubins::TurnParams;
use irontrack::math::routing::optimize_route;
use irontrack::photogrammetry::corridor::{generate_corridor_lines, CorridorParams};
use irontrack::photogrammetry::flightlines::{
    adjust_for_terrain, clip_to_polygon, connect_with_turns, generate_flight_lines,
    generate_lidar_flight_lines, validate_overlap, BoundingBox, FlightPlan, FlightPlanParams,
    LidarPlanParams,
};
use irontrack::photogrammetry::lidar::LidarSensorParams;
use irontrack::types::{AltitudeDatum, FlightLine, SensorParams};

use geographiclib_rs::InverseGeodesic;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "irontrack",
    version,
    about = "Open-source flight management and aerial survey planning engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Query terrain elevation at a geographic coordinate.
    ///
    /// Fetches the Copernicus DEM GLO-30 tile on first use. If the tile is
    /// missing from the local cache it is downloaded automatically from the
    /// Copernicus AWS S3 bucket.
    ///
    /// WARNING: Copernicus DEM is a Digital Surface Model (DSM), not a
    /// bare-earth DTM. AGL clearance over vegetation may be understated by
    /// 2-8 m depending on forest type.
    Terrain {
        /// Geodetic latitude in decimal degrees (−90 … +90).
        #[arg(long)]
        lat: f64,
        /// Geodetic longitude in decimal degrees (−180 … +180).
        #[arg(long)]
        lon: f64,
    },

    /// Generate a photogrammetric survey flight plan.
    ///
    /// Produces a GeoPackage (.gpkg) with the flight lines as LINESTRING
    /// features and, optionally, a GeoJSON file for web map preview.
    ///
    /// Example (Phantom 4 Pro, 3 cm GSD, north–south lines):
    ///
    ///   irontrack plan \
    ///     --min-lat 51.0 --min-lon 0.0 \
    ///     --max-lat 51.1 --max-lon 0.1 \
    ///     --gsd-cm 3.0 --sensor phantom4pro \
    ///     --output plan.gpkg --geojson plan.geojson
    Plan {
        // --- Bounding box ---------------------------------------------------
        /// Minimum (south) latitude in decimal degrees.
        /// Required unless --boundary is provided.
        #[arg(long, default_value = "0.0")]
        min_lat: f64,
        /// Minimum (west) longitude in decimal degrees.
        /// Required unless --boundary is provided.
        #[arg(long, default_value = "0.0")]
        min_lon: f64,
        /// Maximum (north) latitude in decimal degrees.
        /// Required unless --boundary is provided.
        #[arg(long, default_value = "0.0")]
        max_lat: f64,
        /// Maximum (east) longitude in decimal degrees.
        /// Required unless --boundary is provided.
        #[arg(long, default_value = "0.0")]
        max_lon: f64,

        /// KML/KMZ boundary file (alternative to --min-lat/--max-lat etc.).
        /// If provided, the survey area is defined by the polygon(s) in the
        /// file and the bbox flags are ignored.
        #[arg(long, value_name = "PATH")]
        boundary: Option<PathBuf>,

        // --- Sensor ---------------------------------------------------------
        /// Sensor preset. Choices: phantom4pro, mavic3, ixm100.
        /// Ignored when --focal-length-mm is supplied.
        #[arg(long, default_value = "phantom4pro", value_name = "PRESET")]
        sensor: String,

        /// Custom focal length in mm (requires all other --sensor-* args too).
        #[arg(long, requires_all = ["sensor_width_mm","sensor_height_mm","image_width_px","image_height_px"])]
        focal_length_mm: Option<f64>,
        /// Custom sensor width in mm.
        #[arg(long)]
        sensor_width_mm: Option<f64>,
        /// Custom sensor height in mm.
        #[arg(long)]
        sensor_height_mm: Option<f64>,
        /// Custom image width in pixels.
        #[arg(long)]
        image_width_px: Option<u32>,
        /// Custom image height in pixels.
        #[arg(long)]
        image_height_px: Option<u32>,

        // --- Photogrammetry -------------------------------------------------
        /// Target ground sample distance in cm/px (e.g. 3.0 → 3 cm/px).
        #[arg(long, default_value = "5.0", value_name = "CM")]
        gsd_cm: f64,

        /// Lateral (cross-track / side) image overlap in percent.
        #[arg(long, default_value = "60.0", value_name = "PCT")]
        side_lap: f64,

        /// Along-track (forward / end) image overlap in percent.
        #[arg(long, default_value = "80.0", value_name = "PCT")]
        end_lap: f64,

        /// Flight direction in degrees clockwise from north (0 = N–S, 90 = E–W).
        #[arg(long, default_value = "0.0", value_name = "DEG")]
        azimuth: f64,

        /// Planned MSL altitude in metres.
        /// If omitted, the AGL required to achieve the target GSD is used
        /// (assumes flat terrain at 0 m MSL).
        #[arg(long)]
        altitude_msl: Option<f64>,

        // --- LiDAR ----------------------------------------------------------
        /// Mission type: camera (default) or lidar.
        #[arg(long, default_value = "camera", value_name = "TYPE")]
        mission_type: String,

        /// LiDAR pulse repetition rate in Hz (e.g. 240000 for 240 kHz).
        #[arg(long, value_name = "HZ")]
        lidar_prr: Option<f64>,

        /// LiDAR mirror scan rate in Hz.
        #[arg(long, value_name = "HZ")]
        lidar_scan_rate: Option<f64>,

        /// LiDAR total angular field of view in degrees.
        #[arg(long, value_name = "DEG")]
        lidar_fov: Option<f64>,

        /// Target point density in pts/m² (LiDAR only).
        #[arg(long, value_name = "PTS")]
        target_density: Option<f64>,

        // --- Corridor -------------------------------------------------------
        /// Planning mode: area (default) or corridor.
        /// Corridor mode generates flight lines following a KML centerline
        /// polyline instead of filling a bounding box.
        #[arg(long, default_value = "area", value_name = "MODE")]
        mode: String,

        /// KML/KMZ centerline file for corridor mode (LineString geometry).
        #[arg(long, value_name = "PATH")]
        centerline: Option<PathBuf>,

        /// Total corridor width in metres (corridor mode only).
        #[arg(long, value_name = "METRES")]
        corridor_width: Option<f64>,

        /// Number of parallel flight lines: 1, 3, or 5 (corridor mode only).
        #[arg(long, value_name = "N")]
        corridor_passes: Option<u8>,

        // --- Turns ----------------------------------------------------------
        /// Maximum bank angle during procedure turns in degrees.
        /// Higher angles produce tighter turns but increase structural load
        /// and tilt the sensor away from nadir.
        #[arg(long, default_value = "20.0", value_name = "DEG")]
        max_bank_angle: f64,

        /// True airspeed for turn radius computation in m/s.
        /// Default is a typical survey cruise speed.
        #[arg(long, default_value = "30.0", value_name = "M/S")]
        turn_airspeed: f64,

        // --- Route optimization ---------------------------------------------
        /// Enable TSP route optimization to minimize repositioning distance.
        /// Reorders flight lines using nearest-neighbor + 2-opt heuristic.
        #[arg(long)]
        optimize_route: bool,

        /// Launch point latitude in decimal degrees (for route optimization).
        #[arg(long, value_name = "DEG")]
        launch_lat: Option<f64>,

        /// Launch point longitude in decimal degrees (for route optimization).
        #[arg(long, value_name = "DEG")]
        launch_lon: Option<f64>,

        // --- Terrain --------------------------------------------------------
        /// Run terrain-aware validation and altitude adjustment using cached
        /// Copernicus DEM tiles. If a required tile is not in the local cache
        /// it will be downloaded automatically. The flat-terrain plan is used
        /// as a fallback if adjustment fails.
        #[arg(long)]
        terrain: bool,

        /// Target vertical datum for output altitudes. Choices: egm2008, egm96, ellipsoidal, agl.
        /// Default is egm2008 (matches Copernicus DEM).
        #[arg(long, default_value = "egm2008", value_name = "DATUM")]
        datum: String,

        // --- Output ---------------------------------------------------------
        /// GeoPackage output file path.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,

        /// GeoJSON output file path (optional; useful for web map preview).
        #[arg(long, value_name = "PATH")]
        geojson: Option<PathBuf>,

        /// QGroundControl .plan output file path (optional; for ArduPilot/PX4 autopilots).
        #[arg(long, value_name = "PATH")]
        qgc_plan: Option<PathBuf>,

        /// DJI .kmz output file path (optional; for DJI Enterprise autopilots).
        /// Altitudes are automatically converted to EGM96 as required by DJI.
        #[arg(long, value_name = "PATH")]
        dji_kmz: Option<PathBuf>,
    },

    /// Start the IronTrack FMS daemon (Axum server).
    ///
    /// Provides a REST API for mission planning and telemetry.
    Daemon {
        /// TCP port to listen on.
        #[arg(long, default_value = "8080")]
        port: u16,

        /// Enable mock telemetry mode.
        #[arg(long)]
        mock_telemetry: bool,

        /// Speed for mock telemetry (m/s).
        #[arg(long, default_value = "15.0")]
        mock_speed: f64,

        /// USB Vendor ID for GNSS receiver.
        #[arg(long, default_value = "5446")] // 0x1546 (u-blox)
        gnss_vid: u16,

        /// USB Product ID for GNSS receiver.
        #[arg(long, default_value = "425")] // 0x01A9
        gnss_pid: u16,
    },

    /// Ask a question about IronTrack, aerial survey, or photogrammetry.
    ///
    /// Uses the Claude API (requires ANTHROPIC_API_KEY environment variable).
    ///
    /// Example:
    ///
    ///   irontrack ask What GSD do I need for 1:500 mapping?
    Ask {
        /// Your question (free-form text).
        #[arg(trailing_var_arg = true, num_args = 1..)]
        question: Vec<String>,
    },

    /// Convert a natural language mission description into flight plan parameters.
    ///
    /// The AI extracts structured parameters from your description and prints
    /// the equivalent `irontrack plan` command. Add --execute to run it directly.
    ///
    /// Requires ANTHROPIC_API_KEY environment variable.
    ///
    /// Example:
    ///
    ///   irontrack nlplan Survey a 100m x 200m area at 51.5N 0.1W with 3cm GSD using a Phantom 4 Pro
    NlPlan {
        /// Mission description in plain English.
        #[arg(trailing_var_arg = true, num_args = 1..)]
        description: Vec<String>,
        /// Execute the generated plan command immediately instead of just printing it.
        #[arg(long)]
        execute: bool,
        /// GeoPackage output path (required when --execute is used).
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Commands::Terrain { lat, lon }) => run_terrain(lat, lon),
        Some(Commands::Plan {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
            boundary,
            sensor,
            focal_length_mm,
            sensor_width_mm,
            sensor_height_mm,
            image_width_px,
            image_height_px,
            gsd_cm,
            side_lap,
            end_lap,
            azimuth,
            altitude_msl,
            mission_type,
            lidar_prr,
            lidar_scan_rate,
            lidar_fov,
            target_density,
            mode,
            centerline,
            corridor_width,
            corridor_passes,
            max_bank_angle,
            turn_airspeed,
            optimize_route,
            launch_lat,
            launch_lon,
            terrain,
            datum,
            output,
            geojson,
            qgc_plan,
            dji_kmz,
        }) => run_plan(PlanArgs {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
            boundary,
            sensor,
            focal_length_mm,
            sensor_width_mm,
            sensor_height_mm,
            image_width_px,
            image_height_px,
            gsd_cm,
            side_lap,
            end_lap,
            azimuth,
            altitude_msl,
            mission_type,
            lidar_prr,
            lidar_scan_rate,
            lidar_fov,
            target_density,
            mode,
            centerline,
            corridor_width,
            corridor_passes,
            max_bank_angle,
            turn_airspeed,
            optimize_route,
            launch_lat,
            launch_lon,
            terrain,
            datum,
            output,
            geojson,
            qgc_plan,
            dji_kmz,
        }),
        Some(Commands::Daemon {
            port,
            mock_telemetry,
            mock_speed,
            gnss_vid,
            gnss_pid,
        }) => run_daemon(port, mock_telemetry, mock_speed, gnss_vid, gnss_pid),
        Some(Commands::Ask { question }) => run_ask(question.join(" ")),
        Some(Commands::NlPlan {
            description,
            execute,
            output,
        }) => run_nl_plan(description.join(" "), execute, output),
        None => {
            println!("IronTrack v{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Subcommand: daemon
// ---------------------------------------------------------------------------

#[tokio::main]
async fn run_daemon(
    port: u16,
    mock_telemetry: bool,
    mock_speed: f64,
    gnss_vid: u16,
    gnss_pid: u16,
) -> Result<()> {
    let mock = if mock_telemetry {
        Some(mock_speed)
    } else {
        None
    };
    irontrack::network::server::run_server(port, mock, gnss_vid, gnss_pid).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: terrain
// ---------------------------------------------------------------------------

fn run_terrain(lat: f64, lon: f64) -> Result<()> {
    if !(-90.0..=90.0).contains(&lat) {
        bail!("latitude {lat} is out of range (−90 … +90)");
    }
    if !(-180.0..=180.0).contains(&lon) {
        bail!("longitude {lon} is out of range (−180 … +180)");
    }

    let engine = TerrainEngine::new()?;
    let report = engine.query(lat, lon).map_err(|e| match e {
        DemError::TileNotFound(msg) => anyhow::anyhow!("Copernicus DEM tile not found.\n\n{msg}"),
        other => anyhow::anyhow!(other),
    })?;

    if report.source == SeaLevelFallback {
        eprintln!("[Note: Invalid WGS84 coordinate — sea-level terrain assumed]");
    }
    if report.source == OceanFallback {
        eprintln!(
            "[Note: No Copernicus DEM tile for this location (ocean or polar void). \
             Sea-level terrain assumed. Add tidal margin (~1 m) for water operations.]"
        );
    }
    if report.source == CopernicusDem {
        eprintln!(
            "[Note: Copernicus DEM is a surface model (DSM). AGL clearance over \
             vegetation may be understated by 2-8 m.]"
        );
    }

    let lat_label = if lat >= 0.0 {
        format!("{lat:.4}°N")
    } else {
        format!("{:.4}°S", lat.abs())
    };
    let lon_label = if lon >= 0.0 {
        format!("{lon:.4}°E")
    } else {
        format!("{:.4}°W", lon.abs())
    };

    println!("Location: {lat_label}, {lon_label}");
    println!("Orthometric elevation: {:.1} m (MSL)", report.orthometric_m);
    println!("Geoid undulation (N): {:.1} m", report.geoid_undulation_m);
    println!(
        "Ellipsoidal elevation: {:.1} m (WGS84)",
        report.ellipsoidal_m
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: plan
// ---------------------------------------------------------------------------

struct PlanArgs {
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    boundary: Option<PathBuf>,
    sensor: String,
    focal_length_mm: Option<f64>,
    sensor_width_mm: Option<f64>,
    sensor_height_mm: Option<f64>,
    image_width_px: Option<u32>,
    image_height_px: Option<u32>,
    gsd_cm: f64,
    side_lap: f64,
    end_lap: f64,
    azimuth: f64,
    altitude_msl: Option<f64>,
    mission_type: String,
    lidar_prr: Option<f64>,
    lidar_scan_rate: Option<f64>,
    lidar_fov: Option<f64>,
    target_density: Option<f64>,
    mode: String,
    centerline: Option<PathBuf>,
    corridor_width: Option<f64>,
    corridor_passes: Option<u8>,
    max_bank_angle: f64,
    turn_airspeed: f64,
    optimize_route: bool,
    launch_lat: Option<f64>,
    launch_lon: Option<f64>,
    terrain: bool,
    datum: String,
    output: PathBuf,
    geojson: Option<PathBuf>,
    qgc_plan: Option<PathBuf>,
    dji_kmz: Option<PathBuf>,
}

fn run_plan(args: PlanArgs) -> Result<()> {
    // --- Resolve target datum ----------------------------------------------

    let target_datum = match args.datum.to_lowercase().as_str() {
        "egm2008" => AltitudeDatum::Egm2008,
        "egm96" => AltitudeDatum::Egm96,
        "ellipsoidal" | "wgs84" => AltitudeDatum::Wgs84Ellipsoidal,
        "agl" => AltitudeDatum::Agl,
        other => bail!("unknown datum {other:?}. Choose: egm2008, egm96, ellipsoidal, agl."),
    };

    // --- Validate mode and mission type --------------------------------------

    let mode_lower = args.mode.to_lowercase();
    match mode_lower.as_str() {
        "area" | "corridor" => {}
        other => bail!("unknown --mode {other:?}. Choose: area, corridor."),
    }

    let mission_lower = args.mission_type.to_lowercase();
    match mission_lower.as_str() {
        "camera" | "lidar" => {}
        other => bail!("unknown --mission-type {other:?}. Choose: camera, lidar."),
    }

    // --- Corridor mode (early return) ---------------------------------------

    if mode_lower == "corridor" {
        return run_corridor_plan(&args, target_datum);
    }

    // --- Resolve boundary (bbox or KML polygon) ----------------------------

    let boundary_polygons = if let Some(ref boundary_path) = args.boundary {
        let polys = parse_boundary(boundary_path)
            .with_context(|| format!("cannot parse boundary file {}", boundary_path.display()))?;
        if polys.is_empty() {
            bail!("boundary file contains no polygons");
        }
        Some(polys)
    } else {
        None
    };

    /*
     * Build the bounding box. If --boundary was provided, derive it from the
     * polygon's axis-aligned extent. Otherwise use the explicit CLI args.
     * When no boundary is given, validate that the bbox has positive extent
     * (the defaults are all 0.0 which would produce a degenerate box).
     */
    if boundary_polygons.is_none()
        && (args.min_lat == 0.0
            && args.max_lat == 0.0
            && args.min_lon == 0.0
            && args.max_lon == 0.0)
    {
        bail!(
            "either --boundary or all of --min-lat/--max-lat/--min-lon/--max-lon must be provided"
        );
    }

    let bbox = if let Some(ref polys) = boundary_polygons {
        let (mut min_lat, mut min_lon, mut max_lat, mut max_lon) = polys[0].bbox();
        for poly in polys.iter().skip(1) {
            let (a, b, c, d) = poly.bbox();
            min_lat = min_lat.min(a);
            min_lon = min_lon.min(b);
            max_lat = max_lat.max(c);
            max_lon = max_lon.max(d);
        }
        BoundingBox {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
        }
    } else {
        BoundingBox {
            min_lat: args.min_lat,
            min_lon: args.min_lon,
            max_lat: args.max_lat,
            max_lon: args.max_lon,
        }
    };

    // --- Resolve mission type and generate flight lines --------------------

    let is_lidar = mission_lower == "lidar";

    let (plan, params) = if is_lidar {
        /*
         * LiDAR mission: line spacing from swath overlap, not GSD.
         * Require --lidar-prr, --lidar-scan-rate, --lidar-fov.
         */
        let prr = args
            .lidar_prr
            .context("--lidar-prr is required for LiDAR missions")?;
        let scan_rate = args
            .lidar_scan_rate
            .context("--lidar-scan-rate is required for LiDAR missions")?;
        let fov = args
            .lidar_fov
            .context("--lidar-fov is required for LiDAR missions")?;

        let sensor = LidarSensorParams::new(prr, scan_rate, fov, 0.0)
            .context("invalid LiDAR sensor parameters")?;

        /*
         * LiDAR AGL drives swath width and point density. When --altitude-msl
         * is provided, it sets both the AGL (for swath math) and the MSL
         * waypoint elevation. For flat-terrain planning (no DEM), AGL ≈ MSL
         * is a reasonable assumption. Default: 100 m.
         *
         * A dedicated --lidar-agl flag would separate these concerns; for now
         * the altitude_msl value is used directly as AGL, which is correct
         * for the common case of flat-terrain survey planning at low altitude.
         */
        let agl = args.altitude_msl.unwrap_or(100.0);
        let ground_speed = if let Some(density) = args.target_density {
            irontrack::photogrammetry::lidar::required_speed_for_density(&sensor, agl, density)
                .context("cannot compute speed for target density")?
        } else {
            10.0
        };

        let mut lidar_params = LidarPlanParams::new(sensor, agl, ground_speed)
            .context("invalid LiDAR plan parameters")?
            .with_side_lap(args.side_lap)
            .context("invalid side-lap value")?;
        lidar_params.flight_altitude_msl = args.altitude_msl;

        let p = generate_lidar_flight_lines(&bbox, args.azimuth, &lidar_params)
            .context("LiDAR flight line generation failed")?;

        let density = lidar_params.point_density().unwrap_or(0.0);
        println!(
            "LiDAR mission: {:.0} pts/m² at {:.1} m/s, {:.0} m AGL, {:.0}° FOV",
            density, ground_speed, agl, fov
        );

        let pm = p.params.clone();
        (p, pm)
    } else {
        // --- Camera mission (default) --------------------------------------

        let sensor_params = resolve_sensor(&args)?;
        let gsd_m = args.gsd_cm / 100.0;
        let params = FlightPlanParams::new(sensor_params, gsd_m)
            .context("invalid sensor or GSD")?
            .with_side_lap(args.side_lap)
            .context("invalid side-lap value")?
            .with_end_lap(args.end_lap)
            .context("invalid end-lap value")?;

        let params = if let Some(alt) = args.altitude_msl {
            FlightPlanParams {
                flight_altitude_msl: Some(alt),
                ..params
            }
        } else {
            params
        };

        let p = generate_flight_lines(&bbox, args.azimuth, &params)
            .context("flight line generation failed")?;
        let pm = p.params.clone();
        (p, pm)
    };

    // --- Clip to polygon boundary (if provided) ----------------------------

    /*
     * When --boundary is used, the flight lines are generated over the
     * polygon's bounding box, then clipped to the actual polygon shape.
     * For multi-polygon files, each polygon clips the full grid
     * independently and the results are merged.
     */
    let plan = if let Some(ref polys) = boundary_polygons {
        let mut all_lines = Vec::new();
        for poly in polys {
            let clipped = clip_to_polygon(
                FlightPlan {
                    params: plan.params.clone(),
                    azimuth_deg: plan.azimuth_deg,
                    lines: plan.lines.clone(),
                },
                poly,
            );
            all_lines.extend(clipped.lines);
        }
        FlightPlan {
            params: plan.params,
            azimuth_deg: plan.azimuth_deg,
            lines: all_lines,
        }
    } else {
        plan
    };

    println!(
        "Generated {} flight line(s) over {:.4}°N {:.4}°E → {:.4}°N {:.4}°E",
        plan.lines.len(),
        bbox.min_lat,
        bbox.min_lon,
        bbox.max_lat,
        bbox.max_lon,
    );

    // --- Route optimization (optional) ------------------------------------

    let plan = if args.optimize_route && plan.lines.len() >= 2 {
        let launch_lat = args.launch_lat.unwrap_or(bbox.min_lat);
        let launch_lon = args.launch_lon.unwrap_or(bbox.min_lon);
        let result = optimize_route(&plan.lines, launch_lat, launch_lon);
        println!(
            "Route optimized: {:.0} m repositioning ({} lines reordered)",
            result.total_reposition_m,
            result.order.len()
        );

        let mut reordered = Vec::with_capacity(plan.lines.len());
        for (idx, &line_idx) in result.order.iter().enumerate() {
            if result.reversed[idx] {
                let orig = &plan.lines[line_idx];
                let mut rev = FlightLine::new().with_datum(orig.altitude_datum);
                for i in (0..orig.len()).rev() {
                    rev.push(orig.lats()[i], orig.lons()[i], orig.elevations()[i]);
                }
                reordered.push(rev);
            } else {
                reordered.push(plan.lines[line_idx].clone());
            }
        }
        FlightPlan {
            params: plan.params,
            azimuth_deg: plan.azimuth_deg,
            lines: reordered,
        }
    } else {
        plan
    };

    // --- Connect flight lines with Dubins turns ---------------------------

    let plan = if !is_lidar && plan.lines.len() >= 2 {
        let turn_params = validate_turn_params(args.turn_airspeed, args.max_bank_angle)?;
        let r = turn_params.min_radius();
        println!(
            "Turn radius: {:.0} m (V={:.0} m/s, bank={:.0}°)",
            r, args.turn_airspeed, args.max_bank_angle
        );
        connect_with_turns(plan, &turn_params)
    } else {
        plan
    };

    // --- Terrain-aware validation and adjustment (optional) ----------------

    /*
     * Terrain adjustment uses camera-specific swath footprint sampling
     * (5-point per exposure). For LiDAR missions this doesn't apply —
     * LiDAR terrain following is handled differently (continuous swath,
     * not discrete exposures). Skip terrain adjustment for LiDAR.
     */
    let (mut plan, engine) = if args.terrain && !is_lidar {
        apply_terrain_adjustment(plan, &params)?
    } else {
        if args.terrain && is_lidar {
            eprintln!("[Note] Terrain adjustment not yet supported for LiDAR missions.");
        }
        (plan, None)
    };

    // --- DSM safety warning ------------------------------------------------

    /*
     * Determine whether a DSM warning is needed before the datum conversion
     * block that may partially move `engine`. DSM_WARNING_TEXT is 'static so
     * dsm_warning outlives the engine regardless of what happens to it below.
     *
     * If the terrain engine used a DSM (currently always the case for
     * Copernicus), emit the mandatory collision-risk warning to stderr and
     * record the warning text in all output artefacts. This cannot be
     * suppressed: X-band canopy penetration creates a real AGL underestimation
     * risk. The warning is gated on dem_type so future bare-earth DTM sources
     * do not trigger it.
     */
    /*
     * Two paths can access DSM (Copernicus) terrain data:
     *   (a) terrain adjustment was used — engine is Some and always DSM
     *   (b) datum conversion to/from AGL — to_datum() fetches DEM elevations
     *       when source or target datum is AGL; any TerrainEngine created
     *       here is also Copernicus and therefore DSM
     * Both paths must emit the warning. `dsm_warning` is a 'static reference
     * so it outlives any engine move that happens in the datum block below.
     */
    let agl_datum_conversion_uses_dem = plan.lines.iter().any(|l| l.altitude_datum != target_datum)
        && (target_datum == AltitudeDatum::Agl
            || plan
                .lines
                .iter()
                .any(|l| l.altitude_datum == AltitudeDatum::Agl));

    let dsm_warning: Option<&str> = if engine
        .as_ref()
        .is_some_and(|e| e.dem_type() == DemType::Dsm)
        || agl_datum_conversion_uses_dem
    {
        Some(legal::DSM_WARNING_TEXT)
    } else {
        None
    };

    // --- Convert to target vertical datum ----------------------------------

    if plan.lines.iter().any(|l| l.altitude_datum != target_datum) {
        let engine = match engine {
            Some(e) => e,
            None => TerrainEngine::new()
                .context("cannot initialise terrain engine for datum conversion")?,
        };

        let mut converted_lines = Vec::with_capacity(plan.lines.len());
        for line in &plan.lines {
            converted_lines.push(line.to_datum(target_datum, &engine)?);
        }
        plan.lines = converted_lines;
        println!("Altitude datum conversion: applied (target: {target_datum}).");
    }

    // --- Copernicus attribution (when DSM data was used) -------------------

    /*
     * Attribution is legally required in any product derived from Copernicus
     * DEM data. Gate on the same condition as the DSM warning — only inject
     * into output artefacts when terrain or AGL datum conversion actually
     * accessed Copernicus tiles.
     */
    let attribution_string = legal::copernicus_attribution_default();
    let (attribution, disclaimer): (Option<&str>, Option<&str>) = if dsm_warning.is_some() {
        (
            Some(attribution_string.as_str()),
            Some(legal::COPERNICUS_DISCLAIMER),
        )
    } else {
        (None, None)
    };

    // --- Export to GeoPackage ----------------------------------------------

    let gpkg = GeoPackage::new(&args.output)
        .with_context(|| format!("cannot create GeoPackage at {}", args.output.display()))?;
    gpkg.create_feature_table("flight_lines")
        .context("failed to create flight_lines feature table")?;
    gpkg.insert_flight_plan("flight_lines", &plan)
        .context("failed to insert flight plan into GeoPackage")?;

    if let Some(warning) = dsm_warning {
        gpkg.upsert_metadata("safety_dsm_warning", warning)
            .context("failed to write DSM warning to GeoPackage metadata")?;
        gpkg.append_content_description("flight_lines", &format!("; {warning}"))
            .context("failed to append DSM warning to gpkg_contents description")?;
        gpkg.upsert_metadata("copernicus_attribution", &attribution_string)
            .context("failed to write Copernicus attribution to GeoPackage metadata")?;
        gpkg.upsert_metadata("copernicus_disclaimer", legal::COPERNICUS_DISCLAIMER)
            .context("failed to write Copernicus disclaimer to GeoPackage metadata")?;
    }

    println!("GeoPackage written: {}", args.output.display());

    // --- Export to GeoJSON (optional) --------------------------------------

    if let Some(geojson_path) = &args.geojson {
        write_geojson(geojson_path, &plan, dsm_warning, attribution, disclaimer)
            .with_context(|| format!("cannot write GeoJSON to {}", geojson_path.display()))?;
        println!("GeoJSON written:    {}", geojson_path.display());
    }

    // --- Export to QGroundControl .plan (optional) -------------------------

    /*
     * ArduPilot's MAV_FRAME_GLOBAL interprets altitude as WGS84 ellipsoidal.
     * If the plan is in a different non-AGL datum (e.g. EGM2008), the QGC
     * export must convert to WGS84 ellipsoidal first. AGL plans are left
     * as-is since they use MAV_FRAME_GLOBAL_RELATIVE_ALT.
     */
    if let Some(qgc_path) = &args.qgc_plan {
        if is_lidar {
            bail!(
                "--qgc-plan is not supported for LiDAR missions. \
                   QGC camera trigger commands do not apply to LiDAR sensors."
            );
        }
        let trigger_dist = params.photo_interval();
        let qgc_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);

        /*
         * ArduPilot MAV_FRAME_GLOBAL = WGS84 ellipsoidal. If the plan is
         * in a geoid-based datum (EGM2008, EGM96), convert to ellipsoidal
         * before writing. AGL plans are passed through as-is.
         */
        let needs_conversion =
            qgc_datum != AltitudeDatum::Agl && qgc_datum != AltitudeDatum::Wgs84Ellipsoidal;

        let qgc_lines: Option<Vec<_>> = if needs_conversion {
            let engine = TerrainEngine::new()
                .context("cannot initialise terrain engine for QGC datum conversion")?;
            let mut converted = Vec::with_capacity(plan.lines.len());
            for line in &plan.lines {
                converted.push(
                    line.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
                        .context("QGC datum conversion to WGS84 ellipsoidal failed")?,
                );
            }
            Some(converted)
        } else {
            None
        };

        /*
         * Build a temporary FlightPlan referencing the converted lines if
         * conversion was needed, or borrow the original plan's lines.
         */
        let qgc_plan = FlightPlan {
            params: plan.params.clone(),
            azimuth_deg: plan.azimuth_deg,
            lines: qgc_lines.unwrap_or_else(|| plan.lines.clone()),
        };

        write_qgc_plan(qgc_path, &qgc_plan, trigger_dist)
            .with_context(|| format!("cannot write QGC plan to {}", qgc_path.display()))?;
        let exported_datum = qgc_plan
            .lines
            .first()
            .map(|l| l.altitude_datum.as_str())
            .unwrap_or("N/A");
        println!(
            "QGC plan written:   {} (datum: {exported_datum})",
            qgc_path.display()
        );
    }

    // --- Export to DJI .kmz (optional) ------------------------------------

    /*
     * DJI autopilots execute static EGM96 altitudes — no onboard terrain
     * correction. Convert the plan to EGM96 before writing. This is
     * non-negotiable: EGM2008 != EGM96 (0.5–10+ m difference).
     */
    if let Some(dji_path) = &args.dji_kmz {
        if is_lidar {
            bail!(
                "--dji-kmz is not supported for LiDAR missions. \
                   DJI takePhoto actions do not apply to LiDAR sensors."
            );
        }
        let dji_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);

        let dji_plan = if dji_datum != AltitudeDatum::Egm96 {
            let engine = TerrainEngine::new()
                .context("cannot initialise terrain engine for DJI datum conversion")?;
            let mut converted = Vec::with_capacity(plan.lines.len());
            for line in &plan.lines {
                converted.push(
                    line.to_datum(AltitudeDatum::Egm96, &engine)
                        .context("DJI datum conversion to EGM96 failed")?,
                );
            }
            FlightPlan {
                params: plan.params.clone(),
                azimuth_deg: plan.azimuth_deg,
                lines: converted,
            }
        } else {
            FlightPlan {
                params: plan.params.clone(),
                azimuth_deg: plan.azimuth_deg,
                lines: plan.lines.clone(),
            }
        };

        write_dji_kmz(dji_path, &dji_plan)
            .with_context(|| format!("cannot write DJI KMZ to {}", dji_path.display()))?;
        println!("DJI KMZ written:    {} (datum: EGM96)", dji_path.display());
    }

    // --- Print summary, attribution, and DSM warning -----------------------

    /*
     * Output order per spec:
     *   1. Mission summary (stdout)
     *   2. Copernicus attribution (stdout) — only when Copernicus was used
     *   3. DSM safety warning (stderr)    — only when Copernicus was used
     */
    if !is_lidar {
        print_summary(&plan, &params, args.gsd_cm, &args.sensor);
    }

    if dsm_warning.is_some() {
        legal::print_copernicus_attribution();
        /*
         * Flush stdout before writing to stderr. `println!` is line-buffered
         * on terminals but block-buffered on pipes and redirected files, so
         * without an explicit flush the DSM warning can appear before the
         * mission summary or attribution in non-terminal contexts.
         */
        let _ = std::io::stdout().flush();
        legal::print_dsm_warning();
    }

    Ok(())
}

/// Attempt terrain-aware overlap validation + altitude adjustment.
///
/// On DEM tile miss, prints a warning and returns the flat-terrain plan
/// unchanged rather than failing the whole mission. This matches the
/// "graceful degradation" principle — the user gets a plan even without
/// cached tiles, and is clearly warned about the limitation.
fn apply_terrain_adjustment(
    plan: FlightPlan,
    _params: &FlightPlanParams,
) -> Result<(FlightPlan, Option<TerrainEngine>)> {
    let terrain = TerrainEngine::new().context("cannot initialise terrain engine")?;

    /*
     * Phase 1: validate. Report how many exposure points have insufficient
     * overlap due to terrain relief, but don't abort — proceed to adjustment.
     */
    match validate_overlap(&plan, &terrain) {
        Ok(checks) => {
            let violations: Vec<_> = checks.iter().filter(|c| c.violation).collect();
            if violations.is_empty() {
                println!("Terrain validation: OK — no overlap violations detected.");
            } else {
                eprintln!(
                    "[Warning] {} overlap violation(s) detected over terrain. \
                     Applying altitude adjustment.",
                    violations.len()
                );
            }
        }
        Err(DemError::TileNotFound(msg)) => {
            eprintln!("[Warning] Terrain validation skipped — DEM tile not cached.\n  {msg}");
            return Ok((plan, Some(terrain)));
        }
        /*
         * NoData (ocean/void pixel) is not a hard error at the validation
         * phase: the terrain engine already substitutes sea-level (0 m MSL)
         * for water pixels, so the plan remains usable. Treat it the same
         * as a tile miss and proceed with the flat-terrain plan.
         */
        Err(DemError::NoData(_)) => {
            eprintln!(
                "[Warning] Terrain validation skipped — DEM returned no-data (ocean/void pixel)."
            );
            return Ok((plan, Some(terrain)));
        }
        Err(e) => return Err(anyhow::anyhow!(e).context("terrain validation failed")),
    }

    /*
     * Phase 2: adjust. Raise waypoint altitudes over terrain peaks to restore
     * the target side-lap where relief would otherwise cause a shortfall.
     */
    match adjust_for_terrain(&plan, &terrain) {
        Ok(adjusted) => {
            println!("Terrain adjustment:  applied.");
            Ok((adjusted, Some(terrain)))
        }
        Err(DemError::TileNotFound(msg)) => {
            eprintln!("[Warning] Terrain adjustment skipped — DEM tile not cached.\n  {msg}");
            Ok((plan, Some(terrain)))
        }
        Err(DemError::NoData(_)) => {
            eprintln!(
                "[Warning] Terrain adjustment skipped — DEM returned no-data (ocean/void pixel)."
            );
            Ok((plan, Some(terrain)))
        }
        Err(e) => Err(anyhow::anyhow!(e).context("terrain adjustment failed")),
    }
}

// ---------------------------------------------------------------------------
// Corridor plan execution
// ---------------------------------------------------------------------------

/// Execute a corridor survey plan.
///
/// Parses the centerline KML, generates offset flight lines, and exports
/// to the same output formats as area plans.
fn run_corridor_plan(args: &PlanArgs, target_datum: AltitudeDatum) -> Result<()> {
    if args.mission_type.to_lowercase() == "lidar" {
        bail!(
            "corridor mode does not support LiDAR missions. \
             Use --mission-type camera (default) with --mode corridor."
        );
    }

    let centerline_path = args
        .centerline
        .as_ref()
        .context("--centerline is required for corridor mode")?;
    let corridor_width = args
        .corridor_width
        .context("--corridor-width is required for corridor mode")?;
    let corridor_passes = args.corridor_passes.unwrap_or(3);

    // --- Parse centerline from KML ----------------------------------------

    let linestrings = parse_centerline(centerline_path)
        .with_context(|| format!("cannot parse centerline file {}", centerline_path.display()))?;
    if linestrings.is_empty() {
        bail!("centerline file contains no LineString elements");
    }
    let centerline = &linestrings[0];

    // --- Resolve sensor and compute waypoint interval ----------------------

    let sensor_params = resolve_sensor(args)?;
    let gsd_m = args.gsd_cm / 100.0;
    let params = FlightPlanParams::new(sensor_params, gsd_m)
        .context("invalid sensor or GSD")?
        .with_side_lap(args.side_lap)
        .context("invalid side-lap value")?
        .with_end_lap(args.end_lap)
        .context("invalid end-lap value")?;

    let waypoint_interval = params.photo_interval();

    /*
     * Match the area-path datum semantics: explicit --altitude-msl means
     * the altitude is orthometric MSL (tag EGM2008); otherwise the sensor-
     * derived AGL is used (tag Agl).
     */
    let (flight_alt_m, line_datum) = match args.altitude_msl {
        Some(msl) => (msl, AltitudeDatum::Egm2008),
        None => (params.nominal_agl(), AltitudeDatum::Agl),
    };

    // --- Generate corridor flight lines ------------------------------------

    let corridor_params = CorridorParams {
        centerline: centerline.clone(),
        buffer_width: corridor_width,
        num_passes: corridor_passes,
    };

    let lines = generate_corridor_lines(
        &corridor_params,
        waypoint_interval,
        flight_alt_m,
        line_datum,
    )
    .context("corridor flight line generation failed")?;

    /*
     * Compute the reference azimuth from the first segment of the centerline
     * (for metadata purposes — corridor lines follow the polyline, not a
     * fixed azimuth).
     */
    let azimuth_deg = if centerline.len() >= 2 {
        let (lat1, lon1) = centerline[0];
        let (lat2, lon2) = centerline[1];
        let geod = geographiclib_rs::Geodesic::wgs84();
        let (_, azi1, _) = InverseGeodesic::inverse(&geod, lat1, lon1, lat2, lon2);
        ((azi1 % 360.0) + 360.0) % 360.0
    } else {
        0.0
    };

    let plan = FlightPlan {
        params: params.clone(),
        azimuth_deg,
        lines,
    };

    let total_pts: usize = plan.lines.iter().map(|l| l.len()).sum();
    println!(
        "Corridor: {} flight line(s), {} waypoints, {:.0} m width, {} passes",
        plan.lines.len(),
        total_pts,
        corridor_width,
        corridor_passes,
    );

    // --- Route optimization (optional) ------------------------------------

    let plan = if args.optimize_route && plan.lines.len() >= 2 {
        let launch_lat = args
            .launch_lat
            .unwrap_or_else(|| corridor_params.centerline[0].0);
        let launch_lon = args
            .launch_lon
            .unwrap_or_else(|| corridor_params.centerline[0].1);
        let result = optimize_route(&plan.lines, launch_lat, launch_lon);
        println!(
            "Route optimized: {:.0} m repositioning ({} lines reordered)",
            result.total_reposition_m,
            result.order.len()
        );

        let mut reordered = Vec::with_capacity(plan.lines.len());
        for (idx, &line_idx) in result.order.iter().enumerate() {
            if result.reversed[idx] {
                let orig = &plan.lines[line_idx];
                let mut rev = FlightLine::new().with_datum(orig.altitude_datum);
                for i in (0..orig.len()).rev() {
                    rev.push(orig.lats()[i], orig.lons()[i], orig.elevations()[i]);
                }
                reordered.push(rev);
            } else {
                reordered.push(plan.lines[line_idx].clone());
            }
        }
        FlightPlan {
            params: plan.params,
            azimuth_deg: plan.azimuth_deg,
            lines: reordered,
        }
    } else {
        plan
    };

    // --- Connect corridor lines with Dubins turns -------------------------

    let mut plan = if plan.lines.len() >= 2 {
        let turn_params = validate_turn_params(args.turn_airspeed, args.max_bank_angle)?;
        let r = turn_params.min_radius();
        println!(
            "Turn radius: {:.0} m (V={:.0} m/s, bank={:.0}°)",
            r, args.turn_airspeed, args.max_bank_angle
        );
        connect_with_turns(plan, &turn_params)
    } else {
        plan
    };

    // --- Terrain-aware adjustment (optional) -------------------------------

    let engine = if args.terrain {
        match apply_terrain_adjustment(
            FlightPlan {
                params: plan.params.clone(),
                azimuth_deg: plan.azimuth_deg,
                lines: plan.lines.clone(),
            },
            &plan.params,
        ) {
            Ok((adjusted, eng)) => {
                plan = adjusted;
                eng
            }
            Err(e) => {
                eprintln!("[Warning] Terrain adjustment failed: {e:#}");
                None
            }
        }
    } else {
        None
    };

    // --- DSM safety warning ------------------------------------------------

    let agl_datum_conversion_uses_dem = plan.lines.iter().any(|l| l.altitude_datum != target_datum)
        && (target_datum == AltitudeDatum::Agl
            || plan
                .lines
                .iter()
                .any(|l| l.altitude_datum == AltitudeDatum::Agl));

    let dsm_warning: Option<&str> = if engine
        .as_ref()
        .is_some_and(|e| e.dem_type() == DemType::Dsm)
        || agl_datum_conversion_uses_dem
    {
        Some(legal::DSM_WARNING_TEXT)
    } else {
        None
    };

    // --- Convert to target vertical datum ----------------------------------

    if plan.lines.iter().any(|l| l.altitude_datum != target_datum) {
        let engine = match engine {
            Some(e) => e,
            None => TerrainEngine::new()
                .context("cannot initialise terrain engine for datum conversion")?,
        };
        let mut converted_lines = Vec::with_capacity(plan.lines.len());
        for line in &plan.lines {
            converted_lines.push(line.to_datum(target_datum, &engine)?);
        }
        plan.lines = converted_lines;
        println!("Altitude datum conversion: applied (target: {target_datum}).");
    }

    // --- Copernicus attribution -------------------------------------------

    let attribution_string = legal::copernicus_attribution_default();
    let (attribution, disclaimer): (Option<&str>, Option<&str>) = if dsm_warning.is_some() {
        (
            Some(attribution_string.as_str()),
            Some(legal::COPERNICUS_DISCLAIMER),
        )
    } else {
        (None, None)
    };

    // --- Export to GeoPackage ----------------------------------------------

    let gpkg = GeoPackage::new(&args.output)
        .with_context(|| format!("cannot create GeoPackage at {}", args.output.display()))?;
    gpkg.create_feature_table("flight_lines")
        .context("failed to create flight_lines feature table")?;
    gpkg.insert_flight_plan("flight_lines", &plan)
        .context("failed to insert flight plan into GeoPackage")?;

    if let Some(warning) = dsm_warning {
        gpkg.upsert_metadata("safety_dsm_warning", warning)
            .context("failed to write DSM warning to GeoPackage metadata")?;
        gpkg.append_content_description("flight_lines", &format!("; {warning}"))
            .context("failed to append DSM warning to gpkg_contents description")?;
        gpkg.upsert_metadata("copernicus_attribution", &attribution_string)
            .context("failed to write Copernicus attribution to GeoPackage metadata")?;
        gpkg.upsert_metadata("copernicus_disclaimer", legal::COPERNICUS_DISCLAIMER)
            .context("failed to write Copernicus disclaimer to GeoPackage metadata")?;
    }

    println!("GeoPackage written: {}", args.output.display());

    // --- Export to GeoJSON (optional) --------------------------------------

    if let Some(geojson_path) = &args.geojson {
        write_geojson(geojson_path, &plan, dsm_warning, attribution, disclaimer)
            .with_context(|| format!("cannot write GeoJSON to {}", geojson_path.display()))?;
        println!("GeoJSON written:    {}", geojson_path.display());
    }

    // --- Export to QGroundControl .plan (optional) -------------------------

    if let Some(qgc_path) = &args.qgc_plan {
        let trigger_dist = params.photo_interval();
        let qgc_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);

        let needs_conversion =
            qgc_datum != AltitudeDatum::Agl && qgc_datum != AltitudeDatum::Wgs84Ellipsoidal;

        let qgc_lines: Option<Vec<_>> = if needs_conversion {
            let engine = TerrainEngine::new()
                .context("cannot initialise terrain engine for QGC datum conversion")?;
            let mut converted = Vec::with_capacity(plan.lines.len());
            for line in &plan.lines {
                converted.push(
                    line.to_datum(AltitudeDatum::Wgs84Ellipsoidal, &engine)
                        .context("QGC datum conversion to WGS84 ellipsoidal failed")?,
                );
            }
            Some(converted)
        } else {
            None
        };

        let qgc_plan = FlightPlan {
            params: plan.params.clone(),
            azimuth_deg: plan.azimuth_deg,
            lines: qgc_lines.unwrap_or_else(|| plan.lines.clone()),
        };

        write_qgc_plan(qgc_path, &qgc_plan, trigger_dist)
            .with_context(|| format!("cannot write QGC plan to {}", qgc_path.display()))?;
        println!("QGC plan written:   {}", qgc_path.display());
    }

    // --- Export to DJI .kmz (optional) ------------------------------------

    if let Some(dji_path) = &args.dji_kmz {
        let dji_datum = plan
            .lines
            .first()
            .map(|l| l.altitude_datum)
            .unwrap_or(AltitudeDatum::Egm2008);

        let dji_plan = if dji_datum != AltitudeDatum::Egm96 {
            let engine = TerrainEngine::new()
                .context("cannot initialise terrain engine for DJI datum conversion")?;
            let mut converted = Vec::with_capacity(plan.lines.len());
            for line in &plan.lines {
                converted.push(
                    line.to_datum(AltitudeDatum::Egm96, &engine)
                        .context("DJI datum conversion to EGM96 failed")?,
                );
            }
            FlightPlan {
                params: plan.params.clone(),
                azimuth_deg: plan.azimuth_deg,
                lines: converted,
            }
        } else {
            FlightPlan {
                params: plan.params.clone(),
                azimuth_deg: plan.azimuth_deg,
                lines: plan.lines.clone(),
            }
        };

        write_dji_kmz(dji_path, &dji_plan)
            .with_context(|| format!("cannot write DJI KMZ to {}", dji_path.display()))?;
        println!("DJI KMZ written:    {} (datum: EGM96)", dji_path.display());
    }

    // --- Print summary and warnings ----------------------------------------

    if dsm_warning.is_some() {
        legal::print_copernicus_attribution();
        let _ = std::io::stdout().flush();
        legal::print_dsm_warning();
    }

    Ok(())
}

/// Print a human-readable mission summary to stdout.
fn print_summary(plan: &FlightPlan, params: &FlightPlanParams, gsd_cm: f64, sensor_name: &str) {
    let nominal_agl = params.nominal_agl();
    let gsd_achieved_cm = params.sensor.worst_case_gsd(nominal_agl) * 100.0;
    let n_lines = plan.lines.len();
    let total_pts: usize = plan.lines.iter().map(|l| l.len()).sum();

    /*
     * Estimate total flight distance:
     *   along-track leg per line ≈ (pts_per_line − 1) × photo_interval
     *   cross-track ferrying ≈ (n_lines − 1) × line_spacing
     * Add a fixed 30 s turn overhead per line at a typical cruise of 12 m/s.
     */
    let photo_interval = params.photo_interval();
    let line_spacing = params.flight_line_spacing();
    let pts_per_line = if n_lines > 0 { total_pts / n_lines } else { 0 };
    let along_track_per_line_m = (pts_per_line.saturating_sub(1)) as f64 * photo_interval;
    let total_along_m = along_track_per_line_m * n_lines as f64;
    let total_cross_m = if n_lines > 1 {
        (n_lines - 1) as f64 * line_spacing
    } else {
        0.0
    };
    let total_distance_km = (total_along_m + total_cross_m) / 1000.0;

    const CRUISE_SPEED_MS: f64 = 12.0;
    const TURN_TIME_S: f64 = 30.0;
    let flight_time_s = total_distance_km * 1000.0 / CRUISE_SPEED_MS + n_lines as f64 * TURN_TIME_S;
    let flight_time_min = flight_time_s / 60.0;

    println!();
    println!("--- Mission Summary ---");
    println!("  Sensor          : {sensor_name}");
    println!("  Target GSD      : {gsd_cm:.1} cm/px");
    println!("  Achieved GSD    : {gsd_achieved_cm:.2} cm/px");
    println!("  Nominal AGL     : {nominal_agl:.0} m");
    println!("  Side overlap    : {:.0}%", params.side_lap_percent);
    println!("  End overlap     : {:.0}%", params.end_lap_percent);
    println!("  Azimuth         : {:.0}°", plan.azimuth_deg);
    println!();
    println!("  Flight lines    : {n_lines}");
    println!("  Waypoints       : {total_pts}");
    println!("  Line spacing    : {line_spacing:.1} m");
    println!("  Photo interval  : {photo_interval:.1} m");
    println!();
    let datum_str = plan
        .lines
        .first()
        .map(|l| l.altitude_datum.as_str())
        .unwrap_or("EGM2008");

    println!("  Est. distance   : {total_distance_km:.2} km");
    println!("  Est. flight time: {flight_time_min:.0} min  (@ {CRUISE_SPEED_MS:.0} m/s + {TURN_TIME_S:.0}s/turn)");
    println!("  Altitude datum  : {datum_str}");
    println!("----------------------");
}

// ---------------------------------------------------------------------------
// Sensor preset resolution
// ---------------------------------------------------------------------------

/// Built-in sensor presets. Specs sourced from manufacturer datasheets.
///
/// Validate turn parameters and return a `TurnParams` instance.
///
/// Prevents NaN/infinite turn radii from invalid bank angles or airspeeds.
fn validate_turn_params(airspeed: f64, bank_angle: f64) -> Result<TurnParams> {
    if airspeed <= 0.0 || !airspeed.is_finite() {
        bail!("--turn-airspeed must be positive, got {airspeed}");
    }
    if bank_angle <= 0.0 || bank_angle >= 60.0 || !bank_angle.is_finite() {
        bail!("--max-bank-angle must be in (0, 60) degrees, got {bank_angle}");
    }
    Ok(TurnParams::new(airspeed, bank_angle))
}

/// All values are for the primary RGB camera at the widest standard focal
/// length unless otherwise noted.
fn resolve_sensor(args: &PlanArgs) -> Result<SensorParams> {
    /*
     * If any custom sensor dimension was supplied, require all five fields
     * and build a custom SensorParams directly, ignoring the preset.
     */
    if args.focal_length_mm.is_some() {
        return SensorParams::new(
            args.focal_length_mm
                .context("--focal-length-mm is required for a custom sensor")?,
            args.sensor_width_mm
                .context("--sensor-width-mm is required for a custom sensor")?,
            args.sensor_height_mm
                .context("--sensor-height-mm is required for a custom sensor")?,
            args.image_width_px
                .context("--image-width-px is required for a custom sensor")?,
            args.image_height_px
                .context("--image-height-px is required for a custom sensor")?,
        )
        .map_err(|e| anyhow::anyhow!(e));
    }

    match args.sensor.as_str() {
        /*
         * DJI Phantom 4 Pro / Pro V2.0
         * 1-inch CMOS, 20 MP, 8.8 mm lens
         * Source: DJI Phantom 4 Pro product page
         */
        "phantom4pro" => SensorParams::new(8.8, 13.2, 8.8, 5472, 3648),

        /*
         * DJI Mavic 3 (Classic / Enterprise)
         * 4/3 CMOS, 20 MP, Hasselblad L-Format 24 mm equiv = 12.29 mm actual
         * Source: DJI Mavic 3 Enterprise datasheet
         */
        "mavic3" => SensorParams::new(12.29, 17.3, 13.0, 5280, 3956),

        /*
         * Phase One iXM-100 with RSM 50mm lens
         * 100 MP, 53.4×40 mm back-illuminated CMOS
         * Source: Phase One iXM-100 technical datasheet
         */
        "ixm100" => SensorParams::new(50.0, 53.4, 40.0, 11664, 8750),

        other => bail!(
            "unknown sensor preset {other:?}. \
             Choose: phantom4pro, mavic3, ixm100. \
             Or supply --focal-length-mm / --sensor-width-mm / --sensor-height-mm / \
             --image-width-px / --image-height-px for a custom sensor."
        ),
    }
    .map_err(|e| anyhow::anyhow!(e))
}

// ---------------------------------------------------------------------------
// Subcommand: ask
// ---------------------------------------------------------------------------

fn run_ask(question: String) -> Result<()> {
    if question.trim().is_empty() {
        bail!("please provide a question");
    }
    let client = ClaudeClient::from_env().context("cannot initialise Claude API client")?;
    let answer = client
        .send(Some(irontrack::ai::prompts::ASK_SYSTEM_PROMPT), &question)
        .context("Claude API request failed")?;
    println!("{answer}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: nlplan
// ---------------------------------------------------------------------------

fn run_nl_plan(description: String, execute: bool, output: Option<PathBuf>) -> Result<()> {
    if description.trim().is_empty() {
        bail!("please provide a mission description");
    }
    let client = ClaudeClient::from_env().context("cannot initialise Claude API client")?;

    eprintln!("Extracting flight plan parameters from description...");
    let params = client
        .extract_plan_params(&description)
        .context("parameter extraction failed")?;

    print_nl_plan_command(&params);

    if execute {
        let output_path = output.context("--output is required when using --execute")?;
        let plan_args = nl_params_to_plan_args(params, output_path)?;
        run_plan(plan_args)
    } else {
        Ok(())
    }
}

/// Print the equivalent `irontrack plan` CLI command so the user can see
/// exactly what parameters were extracted.
fn print_nl_plan_command(p: &NlPlanParams) {
    let mut cmd = String::from("irontrack plan");

    if let Some(lat) = p.min_lat {
        cmd += &format!(" --min-lat {lat}");
    }
    if let Some(lon) = p.min_lon {
        cmd += &format!(" --min-lon {lon}");
    }
    if let Some(lat) = p.max_lat {
        cmd += &format!(" --max-lat {lat}");
    }
    if let Some(lon) = p.max_lon {
        cmd += &format!(" --max-lon {lon}");
    }
    if let Some(ref s) = p.sensor {
        cmd += &format!(" --sensor {s}");
    }
    if let Some(g) = p.gsd_cm {
        cmd += &format!(" --gsd-cm {g}");
    }
    if let Some(s) = p.side_lap {
        cmd += &format!(" --side-lap {s}");
    }
    if let Some(e) = p.end_lap {
        cmd += &format!(" --end-lap {e}");
    }
    if let Some(a) = p.azimuth {
        cmd += &format!(" --azimuth {a}");
    }
    if p.terrain.unwrap_or(false) {
        cmd += " --terrain";
    }
    if let Some(ref mt) = p.mission_type {
        if mt != "camera" {
            cmd += &format!(" --mission-type {mt}");
        }
    }
    if let Some(alt) = p.altitude_msl {
        cmd += &format!(" --altitude-msl {alt}");
    }
    if let Some(ref d) = p.datum {
        if d != "egm2008" {
            cmd += &format!(" --datum {d}");
        }
    }
    // Custom sensor fields
    if let Some(fl) = p.focal_length_mm {
        cmd += &format!(" --focal-length-mm {fl}");
    }
    if let Some(sw) = p.sensor_width_mm {
        cmd += &format!(" --sensor-width-mm {sw}");
    }
    if let Some(sh) = p.sensor_height_mm {
        cmd += &format!(" --sensor-height-mm {sh}");
    }
    if let Some(iw) = p.image_width_px {
        cmd += &format!(" --image-width-px {iw}");
    }
    if let Some(ih) = p.image_height_px {
        cmd += &format!(" --image-height-px {ih}");
    }
    // LiDAR fields
    if let Some(prr) = p.lidar_prr {
        cmd += &format!(" --lidar-prr {prr}");
    }
    if let Some(sr) = p.lidar_scan_rate {
        cmd += &format!(" --lidar-scan-rate {sr}");
    }
    if let Some(fov) = p.lidar_fov {
        cmd += &format!(" --lidar-fov {fov}");
    }
    if let Some(td) = p.target_density {
        cmd += &format!(" --target-density {td}");
    }

    cmd += " --output <OUTPUT.gpkg>";

    println!("\nEquivalent command:\n  {cmd}\n");
}

/// Convert AI-extracted parameters into the PlanArgs struct used by run_plan().
///
/// Validates mission_type, sensor preset, and datum to prevent LLM
/// hallucinations from silently producing an incorrect plan.
fn nl_params_to_plan_args(p: NlPlanParams, output: PathBuf) -> Result<PlanArgs> {
    let min_lat = p.min_lat.context("AI did not provide min_lat")?;
    let min_lon = p.min_lon.context("AI did not provide min_lon")?;
    let max_lat = p.max_lat.context("AI did not provide max_lat")?;
    let max_lon = p.max_lon.context("AI did not provide max_lon")?;

    let mission_type = p.mission_type.unwrap_or_else(|| "camera".into());
    match mission_type.to_lowercase().as_str() {
        "camera" | "lidar" => {}
        other => {
            bail!("AI returned invalid mission_type {other:?}. Expected \"camera\" or \"lidar\".")
        }
    }

    let sensor = p.sensor.unwrap_or_else(|| "phantom4pro".into());
    if sensor != "custom" && p.focal_length_mm.is_none() {
        match sensor.as_str() {
            "phantom4pro" | "mavic3" | "ixm100" => {}
            other => bail!(
                "AI returned unknown sensor preset {other:?}. \
                 Expected: phantom4pro, mavic3, ixm100, or custom."
            ),
        }
    }

    let datum = p.datum.unwrap_or_else(|| "egm2008".into());
    match datum.to_lowercase().as_str() {
        "egm2008" | "egm96" | "ellipsoidal" | "wgs84" | "agl" => {}
        other => bail!(
            "AI returned unknown datum {other:?}. \
             Expected: egm2008, egm96, ellipsoidal, agl."
        ),
    }

    Ok(PlanArgs {
        min_lat,
        min_lon,
        max_lat,
        max_lon,
        boundary: None,
        sensor,
        focal_length_mm: p.focal_length_mm,
        sensor_width_mm: p.sensor_width_mm,
        sensor_height_mm: p.sensor_height_mm,
        image_width_px: p.image_width_px,
        image_height_px: p.image_height_px,
        gsd_cm: p.gsd_cm.unwrap_or(5.0),
        side_lap: p.side_lap.unwrap_or(60.0),
        end_lap: p.end_lap.unwrap_or(80.0),
        azimuth: p.azimuth.unwrap_or(0.0),
        altitude_msl: p.altitude_msl,
        mission_type,
        lidar_prr: p.lidar_prr,
        lidar_scan_rate: p.lidar_scan_rate,
        lidar_fov: p.lidar_fov,
        target_density: p.target_density,
        mode: "area".into(),
        centerline: None,
        corridor_width: None,
        corridor_passes: None,
        max_bank_angle: 20.0,
        turn_airspeed: 30.0,
        optimize_route: false,
        launch_lat: None,
        launch_lon: None,
        terrain: p.terrain.unwrap_or(false),
        datum,
        output,
        geojson: None,
        qgc_plan: None,
        dji_kmz: None,
    })
}
