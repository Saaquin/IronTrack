#![allow(clippy::all)]
#![allow(clippy::needless_return)]

/*
 * Copyright (c) 2006 D.Ineiev <ineiev@yahoo.co.uk>
 * Copyright (c) 2020 Emeric Grange <emeric.grange@gmail.com>
 * Copyright (c) 2025 Micah Chambers <micahc.vt@gmail.com>
 *
 * This software is provided 'as-is', without any express or implied warranty.
 * In no event will the authors be held liable for any damages arising from
 * the use of this software.
 *
 * Permission is granted to anyone to use this software for any purpose,
 * including commercial applications, and to alter it and redistribute it
 * freely, subject to the following restrictions:
 *
 * 1. The origin of this software must not be misrepresented; you must not
 * claim that you wrote the original software. If you use this software
 * in a product, an acknowledgment in the product documentation would be
 * appreciated but is not required.
 * 2. Altered source versions must be plainly marked as such, and must not be
 * misrepresented as being the original software.
 * 3. This notice may not be removed or altered from any source distribution.
 **/

/*
 * This program is designed for the calculation of a geoid undulation at a point
 * whose latitude and longitude is specified.
 *
 * This program is designed to be used with the constants of EGM96 and those of
 * the WGS84(g873) system. The undulation will refer to the WGS84 ellipsoid.
 *
 * It's designed to use the potential coefficient model EGM96 and a set of
 * spherical harmonic coefficients of a correction term.
 * The correction term is composed of several different components, the primary
 * one being the conversion of a height anomaly to a geoid undulation.
 * The principles of this procedure were initially described in the paper:
 * - use of potential coefficient models for geoid undulation determination using
 * a spherical harmonic representation of the height anomaly/geoid undulation
 * difference by R.H. Rapp, Journal of Geodesy, 1996.
 *
 * This program is a modification of the program described in the following report:
 * - a fortran program for the computation of gravimetric quantities from high
 * degree spherical harmonic expansions, Richard H. Rapp, report 334, Department
 * of Geodetic Science and Surveying, the Ohio State University, Columbus, 1982
 **/

use std::f64::consts::PI;
use std::sync::OnceLock;

use crate::egm96_data::EGM96_DATA;

/****************************************************************************/

// Maximum degree and orders of harmonic coefficients
const NMAX: usize = 360;
const NMAX1: usize = 361;
const N361: usize = 361;
// Size of correction and harmonic coefficients arrays (361*181)
const COEFFS: usize = 65341;

#[cfg(feature = "raster_15_min")]
const EGM96_15_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/egm96-15.png"));

#[cfg(feature = "raster_5_min")]
const EGM96_5_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/egm96-5.png"));

/***************************************************************************/

/// Compute sine and cosine values for the given longitude
fn dscml(rlon: f64, sinml: &mut [f64; N361 + 1], cosml: &mut [f64; N361 + 1]) {
    let a = rlon.sin();
    let b = rlon.cos();

    sinml[1] = a;
    cosml[1] = b;
    sinml[2] = 2.0 * b * a;
    cosml[2] = 2.0 * b * b - 1.0;

    for m in 3..=NMAX {
        sinml[m] = 2.0 * b * sinml[m - 1] - sinml[m - 2];
        cosml[m] = 2.0 * b * cosml[m - 1] - cosml[m - 2];
    }
}

/// Compute height undulation based on coefficients
fn hundu(
    p: &[f64; COEFFS + 1],
    sinml: &[f64; N361 + 1],
    cosml: &[f64; N361 + 1],
    gr: f64,
    re: f64,
) -> f64 {
    // WGS 84 gravitational constant in m^3/s^2 (mass of Earth's atmosphere included)
    const GM: f64 = 0.3986004418e15;
    // WGS 84 datum surface equatorial radius
    const AE: f64 = 6378137.0;

    let ar = AE / re;
    let mut arn = ar;
    let mut ac = 0.0;
    let mut a = 0.0;

    let mut k = 3;
    for n in 2..=NMAX {
        arn *= ar;
        k += 1;
        let mut sum = p[k] * EGM96_DATA[k][2] as f64;
        let mut sumc = p[k] * EGM96_DATA[k][0] as f64;

        for m in 1..=n {
            k += 1;
            let tempc = EGM96_DATA[k][0] as f64 * cosml[m] + EGM96_DATA[k][1] as f64 * sinml[m];
            let temp = EGM96_DATA[k][2] as f64 * cosml[m] + EGM96_DATA[k][3] as f64 * sinml[m];
            sumc += p[k] * tempc;
            sum += p[k] * temp;
        }
        ac += sumc;
        a += sum * arn;
    }
    ac += EGM96_DATA[1][0] as f64
        + (p[2] * EGM96_DATA[2][0] as f64)
        + (p[3] * (EGM96_DATA[3][0] as f64 * cosml[1] + EGM96_DATA[3][1] as f64 * sinml[1]));

    // Add haco = ac/100 to convert height anomaly on the ellipsoid to the undulation
    // Add -0.53m to make undulation refer to the WGS84 ellipsoid

    ((a * GM) / (gr * re)) + (ac / 100.0) - 0.53
}

/// Computes geocentric distance, geocentric latitude, and approximate normal gravity
fn radgra(lat: f64, lon: f64, rlat: &mut f64, gr: &mut f64, re: &mut f64) {
    const A: f64 = 6378137.0;
    const E2: f64 = 0.00669437999013;
    const GEQT: f64 = 9.7803253359;
    const K: f64 = 0.00193185265246;
    let t1 = lat.sin().powi(2);
    let n = A / (1.0 - (E2 * t1)).sqrt();
    let t2 = n * lat.cos();
    let x = t2 * lon.cos();
    let y = t2 * lon.sin();
    let z = (n * (1.0 - E2)) * lat.sin();

    *re = (x * x + y * y + z * z).sqrt(); // compute the geocentric radius
    *rlat = (z / (x * x + y * y).sqrt()).atan(); // compute the geocentric latitude
    *gr = GEQT * (1.0 + (K * t1)) / (1.0 - (E2 * t1)).sqrt(); // compute normal gravity (m/sec²)
}

/// Compute the geoid undulation from the EGM96 model
fn undulation(lat: f64, lon: f64) -> f64 {
    static DRTS_DIRT: OnceLock<([f64; 1301], [f64; 1301])> = OnceLock::new();
    let (drts, dirt) = DRTS_DIRT.get_or_init(|| {
        let nmax2p = (2 * NMAX) + 1;
        let mut drts = [0.0; 1301];
        let mut dirt = [0.0; 1301];
        for n in 1..=nmax2p {
            drts[n] = (n as f64).sqrt();
            dirt[n] = 1.0 / drts[n];
        }

        return (drts, dirt);
    });

    let mut p = [0.0; COEFFS + 1];
    let mut sinml = [0.0; N361 + 1];
    let mut cosml = [0.0; N361 + 1];
    let mut rleg = [0.0; N361 + 1];
    let mut rlnn = [0.0; N361 + 1];

    let mut rlat = 0.0;
    let mut gr = 0.0;
    let mut re = 0.0;

    // Compute the geocentric latitude, geocentric radius, normal gravity
    radgra(lat, lon, &mut rlat, &mut gr, &mut re);
    rlat = (PI / 2.0) - rlat;
    let cothet = rlat.cos();
    let sithet = rlat.sin();

    // compute the legendre functions
    rlnn[1] = 1.0;
    rlnn[2] = sithet * drts[3];
    for j in 1..=NMAX1 {
        let m = j - 1;
        let m1 = m + 1;
        for n1 in 3..=m1 {
            let n = n1 - 1;
            let n2 = 2 * n;
            rlnn[n1] = drts[n2 + 1] * dirt[n2] * sithet * rlnn[n];
        }
    }

    for j in 1..=NMAX1 {
        let m = j - 1;
        let m1 = m + 1;
        let m2 = m + 2;
        let m3 = m + 3;

        if m == 0 {
            rleg[1] = 1.0;
            rleg[2] = cothet * drts[3];
        } else if m == 1 {
            rleg[2] = rlnn[2];
            rleg[3] = drts[5] * cothet * rleg[2];
        }
        rleg[m1] = rlnn[m1];

        if m2 <= NMAX1 {
            rleg[m2] = drts[m1 * 2 + 1] * cothet * rleg[m1];
            for n1 in m3..=NMAX1 {
                let n = n1 - 1;
                if (!m == 0 && n < 2) || (m == 1 && n < 3) {
                    continue;
                }
                let n2 = 2 * n;
                rleg[n1] = drts[n2 + 1]
                    * dirt[n + m]
                    * dirt[n - m]
                    * (drts[n2 - 1] * cothet * rleg[n1 - 1]
                        - drts[n + m - 1] * drts[n - m - 1] * dirt[n2 - 3] * rleg[n1 - 2]);
            }
        }

        for i in j..=NMAX1 {
            p[((i - 1) * i) / 2 + m + 1] = rleg[i];
        }
    }
    dscml(lon, &mut sinml, &mut cosml);

    hundu(&p, &sinml, &cosml, gr, re)
}

fn wrap_degrees(mut degrees: f64) -> f64 {
    degrees += 180.0;
    degrees = degrees.rem_euclid(360.0);
    degrees - 180.0
}

pub fn egm96_compute_altitude_offset(lat: f64, lon: f64) -> f64 {
    let lon = wrap_degrees(lon);
    let lat = lat.clamp(-90.0, 90.0);
    undulation(lat.to_radians(), lon.to_radians())
}

#[allow(unused)]
fn interpolate<const WIDTH: usize, const HEIGHT: usize>(
    lat: f64,
    lon: f64,
    x_start: f64,
    y_start: f64,
    x_step: f64,
    y_step: f64,
    pixels: &[u16],
) -> f64 {
    const SCALE: f64 = 0.003;
    const OFFSET: f64 = -108.0;

    // X_geo = GT(0) + X_pixel * GT(1)
    // Y_geo = GT(3) + Y_line * GT(5)

    // X_pixel = (X_geo - GT(0)) / GT(1)
    // Y_line  = (Y_geo - GT(3)) / GT(5)

    let x = (lon - x_start) / x_step;
    let y = (lat - y_start) / y_step;

    // Determine the integer coordinates surrounding the point.
    let x0 = x.floor() as isize;
    let y0 = y.floor() as isize;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    // Clamp indices so they remain within the image bounds.
    // Convert the floating-point location differences to factors.
    let x0_clamped = x0.clamp(0, (WIDTH - 1) as isize) as usize;
    let y0_clamped = y0.clamp(0, (HEIGHT - 1) as isize) as usize;
    let x1_clamped = x1.clamp(0, (WIDTH - 1) as isize) as usize;
    let y1_clamped = y1.clamp(0, (HEIGHT - 1) as isize) as usize;

    // Compute the fractional part (distance between the point and the floor indices).
    let dx = (x - x0 as f64).clamp(0.0, 1.0);
    let dy = (y - y0 as f64).clamp(0.0, 1.0);

    // Retrieve the values at the four neighboring pixels.
    let top_left = pixels[y0_clamped * WIDTH + x0_clamped] as f64 * SCALE + OFFSET;
    let top_right = pixels[y0_clamped * WIDTH + x1_clamped] as f64 * SCALE + OFFSET;
    let bottom_left = pixels[y1_clamped * WIDTH + x0_clamped] as f64 * SCALE + OFFSET;
    let bottom_right = pixels[y1_clamped * WIDTH + x1_clamped] as f64 * SCALE + OFFSET;

    // Interpolate in the x direction on the top and bottom rows.
    let top = top_left + dx * (top_right - top_left);
    let bottom = bottom_left + dx * (bottom_right - bottom_left);

    // interpolate in the y direction between the top and bottom interpolated values.
    return top + dy * (bottom - top);
}

#[cfg(any(feature = "raster_5_min", feature = "raster_15_min"))]
fn load_image<const WIDTH: usize, const HEIGHT: usize>(bytes: &[u8]) -> Vec<u16> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().expect("Failed to check info");
    // Allocate the output buffer.
    let mut buf = vec![0; reader.output_buffer_size()];
    // Read the next frame. An Atiff might contain multiple frames.
    let info = reader.next_frame(&mut buf).expect("Failed to get frame");

    // Grab the bytes of the image.
    buf.truncate(info.buffer_size());
    assert!(buf.len() == WIDTH * HEIGHT * 2);

    let mut out = vec![0; WIDTH * HEIGHT];
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let index = 2 * (col + row * WIDTH);
            out[row * WIDTH + col] =
                u16::from_be_bytes(buf[index..(index + 2)].try_into().expect("pair"));
        }
    }

    return out;
}

#[cfg(feature = "raster_5_min")]
pub fn egm96_raster_5_min_altitude_offset(lat: f64, lon: f64) -> f64 {
    use std::sync::OnceLock;

    const WIDTH: usize = 4320;
    const HEIGHT: usize = 2161;
    static IMAGE: OnceLock<Vec<u16>> = OnceLock::new();
    let image = IMAGE.get_or_init(|| load_image::<WIDTH, HEIGHT>(EGM96_5_BYTES));

    let mut lon = wrap_degrees(lon);
    if lon < 0.0 {
        lon += 360.0;
    }

    let lat = lat.clamp(-90.0, 90.0);
    // from https://gdal.org/en/stable/tutorials/geotransforms_tut.html
    // X_geo = GT(0) + X_pixel * GT(1) + Y_line * GT(2)
    // Y_geo = GT(3) + X_pixel * GT(4) + Y_line * GT(5)
    // GT(0) = -0.04166666666666666
    // GT(1) = 0.08333333333333333
    // GT(2) = 0
    // GT(3) = 90.04166666666666666
    // GT(4) = 0
    // GT(5) = -0.08333333333333333
    return interpolate::<WIDTH, HEIGHT>(
        lat,
        lon,
        -0.04166666666666666,
        90.04166666666666666,
        0.08333333333333333,
        -0.08333333333333333,
        &image,
    );
}

#[cfg(feature = "raster_15_min")]
pub fn egm96_raster_15_min_altitude_offset(lat: f64, lon: f64) -> f64 {
    use std::sync::OnceLock;

    const WIDTH: usize = 1440;
    const HEIGHT: usize = 721;
    static IMAGE: OnceLock<Vec<u16>> = OnceLock::new();
    let image = IMAGE.get_or_init(|| load_image::<WIDTH, HEIGHT>(EGM96_15_BYTES));

    let mut lon = wrap_degrees(lon);
    if lon < 0.0 {
        lon += 360.0;
    }

    let lat = lat.clamp(-90.0, 90.0);

    // from https://gdal.org/en/stable/tutorials/geotransforms_tut.html
    // X_geo = GT(0) + X_pixel * GT(1) + Y_line * GT(2)
    // Y_geo = GT(3) + X_pixel * GT(4) + Y_line * GT(5)
    // GT(0) = -0.125
    // GT(1) = 0.25
    // GT(2) = 0
    // GT(3) = 90.12500000000000000
    // GT(4) = 0
    // GT(5) = -0.25000000000000000
    return interpolate::<WIDTH, HEIGHT>(lat, lon, -0.125, 90.125, 0.25, -0.25, &image);
}

/// Public function to compute altitude offset using EGM96 model
pub fn egm96_altitude_offset(lat: f64, lon: f64) -> f64 {
    #[cfg(feature = "raster_5_min")]
    {
        // highest res
        egm96_raster_5_min_altitude_offset(lat, lon)
    }

    #[cfg(all(feature = "raster_15_min", not(feature = "raster_5_min")))]
    {
        // medium res
        egm96_raster_15_min_altitude_offset(lat, lon)
    }

    // slow, but little memory
    #[cfg(all(not(feature = "raster_15_min"), not(feature = "raster_5_min")))]
    {
        egm96_compute_altitude_offset(lat, lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn test_radgra_at_equator() {
        let lat = 0.0;
        let lon = 0.0;
        let mut rlat = 0.0;
        let mut gr = 0.0;
        let mut re = 0.0;
        radgra(lat, lon, &mut rlat, &mut gr, &mut re);
        assert!((rlat).abs() < 1e-9); // Geocentric latitude should be close to 0 at equator
        assert!((re - 6378137.0).abs() < 1e-9); // Geocentric radius should be close to semi-major axis
        assert!((gr - 9.7803253359).abs() < 1e-9); // Normal gravity at equator
    }

    #[test]
    fn test_radgra_at_pole() {
        let lat = FRAC_PI_2;
        let lon = 0.0;
        let mut rlat = 0.0;
        let mut gr = 0.0;
        let mut re = 0.0;
        radgra(lat, lon, &mut rlat, &mut gr, &mut re);
        assert!((rlat - FRAC_PI_2).abs() < 1e-9); // Geocentric latitude should be close to pi/2 at pole
        assert!((re - 6356752.314245).abs() < 1e-6); // Geocentric radius at pole (semi-minor axis)
        assert!((gr - 9.8321863685).abs() < 1e-5); // Normal gravity at pole
    }

    #[test]
    fn test_undulation_at_locations() {
        struct Check {
            lat: f64,
            lon: f64,
            geoid: f64,
        }

        let checks = [
            //Houston       :
            Check {
                lat: 29.7604,
                lon: -95.3698,
                geoid: -28.41,
            },
            //San Antonio   :
            Check {
                lat: 29.4241,
                lon: -98.4936,
                geoid: -26.52,
            },
            //San Diego     :
            Check {
                lat: 32.7157,
                lon: -117.1611,
                geoid: -35.22,
            },
            //Dallas        :
            Check {
                lat: 32.7767,
                lon: -96.797,
                geoid: -27.34,
            },
            //San Jose      :
            Check {
                lat: 37.3382,
                lon: -121.8863,
                geoid: -32.37,
            },
            //Los Angeles   :
            Check {
                lat: 34.0522,
                lon: -118.2437,
                geoid: -35.17,
            },
            //New York      :
            Check {
                lat: 40.7128,
                lon: -74.006,
                geoid: -32.73,
            },
            //San Francisco :
            Check {
                lat: 37.7749,
                lon: -122.4194,
                geoid: -32.17,
            },
            //Chicago       :
            Check {
                lat: 41.8781,
                lon: -87.6298,
                geoid: -33.93,
            },
            //London        :
            Check {
                lat: 51.5074,
                lon: 0.1278,
                geoid: 45.78,
            },
            //Paris         :
            Check {
                lat: 48.8566,
                lon: 2.3522,
                geoid: 44.61,
            },
            //Toky          :
            Check {
                lat: 35.6895,
                lon: 139.6917,
                geoid: 36.71,
            },
            //Philadelphia  :
            Check {
                lat: 40.05,
                lon: -75.45,
                geoid: -34.32,
            },
            //Phoenix       :
            Check {
                lat: 33.4484,
                lon: -112.074,
                geoid: -30.25,
            },
            //null island
            Check {
                lat: 0.0,
                lon: 0.0,
                geoid: 17.22,
            },
        ];

        for check in checks {
            let computed = egm96_compute_altitude_offset(check.lat, check.lon);
            let expected = check.geoid;
            let err = (computed - expected).abs();
            if err.is_nan() || err.is_infinite() || err > 0.5 {
                panic!(
                    "Lat: {}, Lon: {}, Expected: {expected}, Computed: {computed}",
                    check.lat, check.lon
                );
            }
        }
    }

    #[test]
    fn test_wrap_degrees() {
        assert_eq!(wrap_degrees(0.0), 0.0);
        assert_eq!(wrap_degrees(179.8), 179.8);
        assert_eq!(wrap_degrees(-179.0), -179.0);
        assert_eq!(wrap_degrees(-181.0), 179.0);
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(-190.0 - 360.0), 170.0);
        assert_eq!(wrap_degrees(360.0), 0.0);
        assert_eq!(wrap_degrees(540.0), -180.0);
        assert_eq!(wrap_degrees(-540.0), -180.0);
        assert_eq!(wrap_degrees(1000.0), -80.0);
        assert_eq!(wrap_degrees(-1000.0), 80.0);
    }

    #[cfg(feature = "raster_5_min")]
    #[test]
    fn test_5min_at_locations() {
        let _ = env_logger::builder().is_test(true).try_init();

        struct Check {
            lat: f64,
            lon: f64,
            geoid: f64,
        }

        let checks = [
            //Houston       :
            Check {
                lat: 29.7604,
                lon: -95.3698,
                geoid: -28.41,
            },
            //San Antonio   :
            Check {
                lat: 29.4241,
                lon: -98.4936,
                geoid: -26.52,
            },
            //San Diego     :
            Check {
                lat: 32.7157,
                lon: -117.1611,
                geoid: -35.22,
            },
            //Dallas        :
            Check {
                lat: 32.7767,
                lon: -96.797,
                geoid: -27.34,
            },
            //San Jose      :
            Check {
                lat: 37.3382,
                lon: -121.8863,
                geoid: -32.37,
            },
            //Los Angeles   :
            Check {
                lat: 34.0522,
                lon: -118.2437,
                geoid: -35.17,
            },
            //New York      :
            Check {
                lat: 40.7128,
                lon: -74.006,
                geoid: -32.73,
            },
            //San Francisco :
            Check {
                lat: 37.7749,
                lon: -122.4194,
                geoid: -32.17,
            },
            //Chicago       :
            Check {
                lat: 41.8781,
                lon: -87.6298,
                geoid: -33.93,
            },
            //London        :
            Check {
                lat: 51.5074,
                lon: 0.1278,
                geoid: 45.78,
            },
            //Paris         :
            Check {
                lat: 48.8566,
                lon: 2.3522,
                geoid: 44.61,
            },
            //Toky          :
            Check {
                lat: 35.6895,
                lon: 139.6917,
                geoid: 36.71,
            },
            //Philadelphia  :
            Check {
                lat: 40.05,
                lon: -75.45,
                geoid: -34.32,
            },
            //Phoenix       :
            Check {
                lat: 33.4484,
                lon: -112.074,
                geoid: -30.25,
            },
            //null island
            Check {
                lat: 0.0,
                lon: 0.0,
                geoid: 17.22,
            },
        ];

        for check in checks {
            let computed = egm96_raster_5_min_altitude_offset(check.lat, check.lon);
            let expected = check.geoid;
            let err = (computed - expected).abs();
            if err.is_nan() || err.is_infinite() || err > 0.5 {
                panic!(
                    "Lat: {}, Lon: {}, Expected: {expected}, Computed: {computed}",
                    check.lat, check.lon
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "raster_15_min")]
    fn test_15min_at_locations() {
        let _ = env_logger::builder().is_test(true).try_init();

        struct Check {
            lat: f64,
            lon: f64,
            geoid: f64,
        }

        let checks = [
            //Houston       :
            Check {
                lat: 29.7604,
                lon: -95.3698,
                geoid: -28.41,
            },
            //San Antonio   :
            Check {
                lat: 29.4241,
                lon: -98.4936,
                geoid: -26.52,
            },
            //San Diego     :
            Check {
                lat: 32.7157,
                lon: -117.1611,
                geoid: -35.22,
            },
            //Dallas        :
            Check {
                lat: 32.7767,
                lon: -96.797,
                geoid: -27.34,
            },
            //San Jose      :
            Check {
                lat: 37.3382,
                lon: -121.8863,
                geoid: -32.37,
            },
            //Los Angeles   :
            Check {
                lat: 34.0522,
                lon: -118.2437,
                geoid: -35.17,
            },
            //New York      :
            Check {
                lat: 40.7128,
                lon: -74.006,
                geoid: -32.73,
            },
            //San Francisco :
            Check {
                lat: 37.7749,
                lon: -122.4194,
                geoid: -32.17,
            },
            //Chicago       :
            Check {
                lat: 41.8781,
                lon: -87.6298,
                geoid: -33.93,
            },
            //London        :
            Check {
                lat: 51.5074,
                lon: 0.1278,
                geoid: 45.78,
            },
            //Paris         :
            Check {
                lat: 48.8566,
                lon: 2.3522,
                geoid: 44.61,
            },
            //Tokyo
            Check {
                lat: 35.355,
                lon: 139.895,
                geoid: 47303.0 * 0.003 - 108.0,
            },
            //Philadelphia  :
            Check {
                lat: 40.05,
                lon: -75.45,
                geoid: -34.32,
            },
            //Phoenix       :
            Check {
                lat: 33.4484,
                lon: -112.074,
                geoid: -30.25,
            },
            //null island
            Check {
                lat: 0.0,
                lon: 0.0,
                geoid: 17.22,
            },
        ];

        for (i, check) in checks.iter().enumerate() {
            let computed = egm96_raster_15_min_altitude_offset(check.lat, check.lon);
            let expected = check.geoid;
            let err = (computed - expected).abs();
            if err.is_nan() || err.is_infinite() || err > 0.5 {
                panic!(
                    "{i}, Lat: {}, Lon: {}, Expected: {expected}, Computed: {computed}",
                    check.lat, check.lon
                );
            }
        }
    }

    // In these tests, we will use a simple 3x3 image.
    // We define WIDTH = 3 and HEIGHT = 3.
    // The pixel array is provided row-major order.
    // For example, we fill the pixels with various values:
    // Row 0: 100, 110, 120
    // Row 1: 130, 140, 150
    // Row 2: 160, 170, 180
    //
    // Also note that the interpolation function applies:
    // value => value * SCALE + OFFSET, where SCALE = 0.003 and OFFSET = -108.
    //
    // Therefore:
    // For pixel 100: 100 * 0.003 - 108 = -107.7
    // For pixel 110: 110 * 0.003 - 108 = -107.67
    // For pixel 120: 120 * 0.003 - 108 = -107.64
    // For pixel 130: 130 * 0.003 - 108 = -107.61
    // For pixel 140: 140 * 0.003 - 108 = -107.58
    // For pixel 150: 150 * 0.003 - 108 = -107.55
    // For pixel 160: 160 * 0.003 - 108 = -107.52
    // For pixel 170: 170 * 0.003 - 108 = -107.49
    // For pixel 180: 180 * 0.003 - 108 = -107.46

    const WIDTH: usize = 3;
    const HEIGHT: usize = 3;

    // Define our test image pixels.
    const PIXELS: [u16; WIDTH * HEIGHT] = [100, 110, 120, 130, 140, 150, 160, 170, 180];

    // We'll use x_start = 0.0 and y_start = 0.0 with steps of 1.0.
    const X_START: f64 = 0.0;
    const Y_START: f64 = 0.0;
    const X_STEP: f64 = 1.0;
    const Y_STEP: f64 = 1.0;

    // A helper function to compare two floating-point numbers within an epsilon.
    fn approx_eq(a: f64, b: f64, epsilon: f64) -> bool {
        (a - b).abs() < epsilon
    }

    #[test]
    fn test_exact_top_left() {
        // Testing a point that exactly maps to the top-left pixel.
        // lat = 0, lon = 0; therefore, x = 0, y = 0.
        let lat = 0.0;
        let lon = 0.0;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        // Expected value is the value of pixel at (0,0): 100 * 0.003 - 108 = -107.7
        let expected = 100.0 * 0.003 - 108.0;
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_exact_bottom_right() {
        // Testing a point that exactly maps to the bottom-right pixel.
        // For a 3x3 image, bottom-right pixel is at (2,2).
        let lat = 2.0;
        let lon = 2.0;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        // Expected value is the value of pixel at (2,2): 180 * 0.003 - 108 = -107.46
        let expected = 180.0 * 0.003 - 108.0;
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_center_interpolation() {
        // Testing a point that lies in the exact center of the top-left 2x2 block.
        // For lat = 0.5 and lon = 0.5, x, y = 0.5, so dx = 0.5 and dy = 0.5.
        //
        // The four neighboring pixel values:
        // top_left: (0,0) => 100 * 0.003 - 108 = -107.7
        // top_right: (1,0) => 110 * 0.003 - 108 = -107.67
        // bottom_left: (0,1) => 130 * 0.003 - 108 = -107.61
        // bottom_right: (1,1) => 140 * 0.003 - 108 = -107.58
        //
        // Interpolating:
        // top = -107.7 + 0.5 * ( -107.67 - (-107.7) ) = -107.7 + 0.5 * 0.03 = -107.685
        // bottom = -107.61 + 0.5 * ( -107.58 - (-107.61) ) = -107.61 + 0.5 * 0.03 = -107.595
        // result = top + 0.5 * (bottom - top) = -107.685 + 0.5 * (0.09) = -107.685 + 0.045 = -107.64
        let lat = 0.5;
        let lon = 0.5;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        let expected = -107.64; // Based on the interpolation above.
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_right_edge_clamping() {
        // Testing a point on the right boundary that may have an x coordinate equal to WIDTH - 1.
        // For lon = 2.0 and lat = 0.0, x = 2.0 and y = 0.0.
        // The surrounding indices become:
        // x0 = 2, x1 = 3 (clamped to 2); y0 = 0, y1 = 1.
        // All x values used in interpolation will refer to column index 2.
        let lat = 0.0;
        let lon = 2.0;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        // Expected value is the pixel at (2,0): 120 * 0.003 - 108 = -107.64.
        let expected = 120.0 * 0.003 - 108.0;
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_negative_coordinates_clamping() {
        // Testing a point with negative lat and lon.
        // For lat = -0.5 and lon = -0.5, the computed x and y are -0.5.
        // x0 = -1 and y0 = -1. After clamping, they become 0.
        // x1 and y1 are then also clamped to 0.
        //
        // Thus, all four neighbors are pixel (0,0).
        let lat = -0.5;
        let lon = -0.5;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        // Expected value is pixel at (0,0): 100 * 0.003 - 108 = -107.7.
        let expected = 100.0 * 0.003 - 108.0;
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_bottom_edge_interpolation() {
        // Testing a point on the lower edge where y is exactly at HEIGHT-1.
        // For lat = 2.0 and lon = 1.5, x = 1.5, y = 2.0.
        // x0 = 1, x1 = 2; y0 = 2, y1 = 3 (clamped to 2).
        // In this case the top and bottom rows are the same (row 2).
        let lat = 2.0;
        let lon = 1.5;
        let result =
            interpolate::<WIDTH, HEIGHT>(lat, lon, X_START, Y_START, X_STEP, Y_STEP, &PIXELS);
        // The interpolation is only in x.
        // For row 2, the pixels at x=1 and x=2 are:
        // pixel at (1,2): 170 * 0.003 - 108 = -107.49
        // pixel at (2,2): 180 * 0.003 - 108 = -107.46
        // x factor = 0.5, so expected = -107.49 + 0.5 * (-107.46 + 107.49) = -107.49 + 0.5 * 0.03 = -107.475
        let expected = -107.475;
        assert!(
            approx_eq(result, expected, 1e-6),
            "Expected {}, got {}",
            expected,
            result
        );
    }
}
