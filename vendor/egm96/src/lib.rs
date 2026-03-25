mod algo;
mod egm96_data;

pub use algo::egm96_altitude_offset;
pub use algo::egm96_compute_altitude_offset;

#[cfg(feature = "raster_15_min")]
pub use algo::egm96_raster_15_min_altitude_offset;

#[cfg(feature = "raster_5_min")]
pub use algo::egm96_raster_5_min_altitude_offset;
