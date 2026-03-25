/*
 * Patched build.rs — Windows compatibility fix.
 *
 * Original used std::os::unix::fs::symlink unconditionally, preventing
 * compilation on Windows even when no raster features are enabled.
 *
 * The PNG files are only needed for raster_5_min / raster_15_min features.
 * Without those features, egm96_altitude_offset falls back to the spherical
 * harmonic egm96_compute_altitude_offset, which uses only the EGM96_DATA
 * static array embedded in egm96_data.rs — no external files required.
 */

fn main() {
    // Nothing to do when no raster features are enabled.
    #[cfg(any(feature = "raster_5_min", feature = "raster_15_min"))]
    {
        panic!(
            "raster features require the fetch-maps feature or EGM96_*_MIN \
             environment variables pointing to the PNG data files. \
             This vendored patch does not auto-download on Windows."
        );
    }
}
