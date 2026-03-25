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

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use irontrack::dem::ElevationSource::{CopernicusDem, OceanFallback, SeaLevelFallback};
use irontrack::dem::TerrainEngine;
use irontrack::error::DemError;
use irontrack::gpkg::GeoPackage;
use irontrack::io::write_geojson;
use irontrack::photogrammetry::flightlines::{
    adjust_for_terrain, generate_flight_lines, validate_overlap, BoundingBox, FlightPlan,
    FlightPlanParams,
};
use irontrack::types::SensorParams;

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
        #[arg(long)]
        min_lat: f64,
        /// Minimum (west) longitude in decimal degrees.
        #[arg(long)]
        min_lon: f64,
        /// Maximum (north) latitude in decimal degrees.
        #[arg(long)]
        max_lat: f64,
        /// Maximum (east) longitude in decimal degrees.
        #[arg(long)]
        max_lon: f64,

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

        // --- Terrain --------------------------------------------------------
        /// Run terrain-aware validation and altitude adjustment using cached
        /// Copernicus DEM tiles. If a required tile is not in the local cache
        /// it will be downloaded automatically. The flat-terrain plan is used
        /// as a fallback if adjustment fails.
        #[arg(long)]
        terrain: bool,

        // --- Output ---------------------------------------------------------
        /// GeoPackage output file path.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,

        /// GeoJSON output file path (optional; useful for web map preview).
        #[arg(long, value_name = "PATH")]
        geojson: Option<PathBuf>,
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
            terrain,
            output,
            geojson,
        }) => run_plan(PlanArgs {
            min_lat,
            min_lon,
            max_lat,
            max_lon,
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
            terrain,
            output,
            geojson,
        }),
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
    terrain: bool,
    output: PathBuf,
    geojson: Option<PathBuf>,
}

fn run_plan(args: PlanArgs) -> Result<()> {
    // --- Resolve sensor parameters -----------------------------------------

    let sensor_params = resolve_sensor(&args)?;

    // --- Build FlightPlanParams --------------------------------------------

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

    // --- Validate and build BoundingBox ------------------------------------

    let bbox = BoundingBox {
        min_lat: args.min_lat,
        min_lon: args.min_lon,
        max_lat: args.max_lat,
        max_lon: args.max_lon,
    };

    // --- Generate flat-terrain flight lines --------------------------------

    let plan = generate_flight_lines(&bbox, args.azimuth, &params)
        .context("flight line generation failed")?;

    println!(
        "Generated {} flight line(s) over {:.4}°N {:.4}°E → {:.4}°N {:.4}°E",
        plan.lines.len(),
        args.min_lat,
        args.min_lon,
        args.max_lat,
        args.max_lon,
    );

    // --- Terrain-aware validation and adjustment (optional) ----------------

    let plan = if args.terrain {
        apply_terrain_adjustment(plan, &params)?
    } else {
        plan
    };

    // --- Export to GeoPackage ----------------------------------------------

    let gpkg = GeoPackage::new(&args.output)
        .with_context(|| format!("cannot create GeoPackage at {}", args.output.display()))?;
    gpkg.create_feature_table("flight_lines")
        .context("failed to create flight_lines feature table")?;
    gpkg.insert_flight_plan("flight_lines", &plan)
        .context("failed to insert flight plan into GeoPackage")?;

    println!("GeoPackage written: {}", args.output.display());

    // --- Export to GeoJSON (optional) --------------------------------------

    if let Some(geojson_path) = &args.geojson {
        write_geojson(geojson_path, &plan)
            .with_context(|| format!("cannot write GeoJSON to {}", geojson_path.display()))?;
        println!("GeoJSON written:    {}", geojson_path.display());
    }

    // --- Print summary -----------------------------------------------------

    print_summary(&plan, &params, args.gsd_cm, &args.sensor);

    Ok(())
}

/// Attempt terrain-aware overlap validation + altitude adjustment.
///
/// On DEM tile miss, prints a warning and returns the flat-terrain plan
/// unchanged rather than failing the whole mission. This matches the
/// "graceful degradation" principle — the user gets a plan even without
/// cached tiles, and is clearly warned about the limitation.
fn apply_terrain_adjustment(plan: FlightPlan, _params: &FlightPlanParams) -> Result<FlightPlan> {
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
            return Ok(plan);
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
            return Ok(plan);
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
            Ok(adjusted)
        }
        Err(DemError::TileNotFound(msg)) => {
            eprintln!("[Warning] Terrain adjustment skipped — DEM tile not cached.\n  {msg}");
            Ok(plan)
        }
        Err(DemError::NoData(_)) => {
            eprintln!(
                "[Warning] Terrain adjustment skipped — DEM returned no-data (ocean/void pixel)."
            );
            Ok(plan)
        }
        Err(e) => Err(anyhow::anyhow!(e).context("terrain adjustment failed")),
    }
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
    println!("  Est. distance   : {total_distance_km:.2} km");
    println!("  Est. flight time: {flight_time_min:.0} min  (@ {CRUISE_SPEED_MS:.0} m/s + {TURN_TIME_S:.0}s/turn)");
    println!("----------------------");
}

// ---------------------------------------------------------------------------
// Sensor preset resolution
// ---------------------------------------------------------------------------

/// Built-in sensor presets. Specs sourced from manufacturer datasheets.
///
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
