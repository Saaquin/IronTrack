# Comprehensive Technical Analysis of the Earth Gravitational Model 2008 (EGM2008): Grid Formats, Interpolation, and Computational Implementations

## Introduction to the Earth Gravitational Model 2008

The Earth Gravitational Model 2008 (EGM2008) represents a monumental paradigm shift in the fields of physical geodesy, global gravity field modeling, and geospatial intelligence. Released by the U.S. National Geospatial-Intelligence Agency (NGA) EGM Development Team, EGM2008 defines the geopotential of the Earth using a fully normalized spherical harmonic expansion complete to degree and order 2159, with additional spherical harmonic coefficients extending to degree 2190 and order 2159.1 This mathematical equipotential surface provides the geoid—a representation of the Earth's gravity field that most closely approximates global mean sea level in the absence of wind, tides, and oceanographic dynamics.4 By establishing the geoid undulation (N)—the precise vertical separation between the geoid and a reference ellipsoid (specifically the World Geodetic System 1984, or WGS 84)—geodetic and navigation systems can translate geometric ellipsoidal heights (h) obtained from Global Navigation Satellite Systems (GNSS) into physical orthometric heights (H) via the fundamental geodetic relationship h = H + N.6

Prior to the public release of EGM2008, the international geodetic standard was the Earth Gravitational Model 1996 (EGM96). EGM96 was a composite solution limited to a spherical harmonic degree and order of 360, which yielded a spatial resolution of approximately 111 kilometers at the equator.8 In profound contrast, EGM2008 incorporates highly accurate satellite gravimetry from the Gravity Recovery and Climate Experiment (GRACE) mission, merged with altimetry-derived and densely sampled terrestrial gravity anomalies on a 5 arc-minute equiangular grid.10 Over areas where only lower-resolution gravity data were available, their spectral content was supplemented with gravitational information implied by the topography using Residual Terrain Modeling (RTM).12 This deep data integration allows EGM2008 to achieve a spatial sampling resolution of approximately 9 kilometers (5 arc-minutes) at the equator, representing an improvement in spatial resolution by a factor of six, and an accuracy enhancement by factors of three to six over its predecessor.9

While the spherical harmonic coefficients define the continuous mathematical model, synthesizing these coefficients into geoid undulations at arbitrary coordinates requires immense computational overhead. To facilitate computational efficiency and widespread integration into Geographic Information Systems (GIS), the NGA provides pre-computed geoid undulation grids derived from the EGM2008 spherical harmonic coefficients.3 These global grids, typically distributed at 1-arcminute and 2.5-arcminute resolutions, bypass the need for end-users to perform computationally expensive spherical harmonic synthesis.15 This report provides an exhaustive technical analysis of these pre-computed EGM2008 grids, interrogating their exact binary file formats, interpolation mechanics, regional accuracy discrepancies compared to legacy models, geodetic tide system specifications, and the memory footprints required by modern open-source software implementations.

## Exact Binary File Formats and Grid Layouts

The distribution and integration of EGM2008 geoid grids rely on several highly specific binary file formats. Because a 1-arcminute global grid encompasses the entire surface of the Earth, it contains hundreds of millions of discrete data points. Standardizing the byte layout, endianness, and header structure is critical for accurate geospatial software development, as a single byte-shift or endianness mismatch will result in catastrophic data misalignment.

### Grid Spacing and Matrix Dimensions

The NGA distributes the pre-computed EGM2008 grids in two primary spatial resolutions: 2.5 arc-minutes and 1 arc-minute.15 The 2.5-arcminute grid provides a node every ~4.6 kilometers at the equator, while the 1-arcminute grid provides a much denser node every ~1.85 kilometers. Both grids cover the entire globe, spanning from 90° North to 90° South in latitude, and from 0° to 360° East in longitude.17

The mathematical dimensions of these grids are absolute and define the exact byte sizes of the resulting binary files. For the 2.5-arcminute grid, the latitudinal extent requires (180/2.5×60)+1 = 4,321 rows, and the longitudinal extent requires (360/2.5×60) = 8,640 columns. For the extremely dense 1-arcminute grid, the dimensions scale to (180×60)+1 = 10,801 rows and (360×60) = 21,600 columns.17 Consequently, a 1-arcminute global grid file stores exactly 233,301,600 floating-point geoid undulation values.18

### The NGA Raw Binary Format (.grd)

The primary 1-arcminute EGM2008 grid file officially distributed by the NGA (typically named Und_min1x1_egm2008_isw=82_WGS84_TideFree.gz or similar .grd variants) contains the pre-computed undulations in a strictly ordered, bare-metal binary format.17 The data type utilized for the geoid undulation values is a 32-bit (4-byte) IEEE 754 single-precision floating-point number (float32).17

A critical operational nuance of the official NGA binary file is its structural origin in legacy Fortran-77 computing environments. Fortran sequential (unformatted) I/O prepends and appends a 4-byte integer record-length marker to every write operation. The file does not feature a conventional metadata header containing bounding box coordinates or grid spacing identifiers; rather, it is a raw binary file written using Fortran's unformatted sequential access protocol.17 In this specific Fortran format, each record (which corresponds to one entire latitudinal row of floating-point numbers) is strictly encapsulated by hidden byte markers. Specifically, the Fortran compiler inserts a 4-byte integer at the beginning and the end of every row to indicate the record's length in bytes.

For the 1-arcminute grid, a single row contains 21,600 float32 values, which equates to exactly 86,400 bytes of data. Therefore, the binary layout per row is rigidly structured as follows:

- A 4-byte integer padding marker containing the precise numerical value 86400.

- The 86,400 bytes of actual float32 geoid undulation values (ordered sequentially from West to East).

- A trailing 4-byte integer padding marker also containing the numerical value 86400.17

This Fortran-specific pattern repeats 10,801 times, stepping row-by-row from 90° North down to 90° South. Parsers written in modern languages such as C++, Python, or Rust must actively seek, read, and discard these 8-byte-per-row padding markers. Failing to account for these Fortran record markers results in immediate off-by-one errors, destroying the floating-point alignment for all subsequent rows.17

Furthermore, the endianness of the original NGA files is natively Big-Endian, which was the standard for high-performance computing clusters at the time of the model's development. Because most modern consumer and server architectures (x86 and ARM) operate on Little-Endian architecture, raw ingestion of the NGA Big-Endian file requires continuous byte-swapping at runtime. To alleviate this computational overhead, the NGA subsequently released Little-Endian versions of the binary files, which are explicitly denoted with _SE (Small Endian) in the filename (e.g., Eg2008_to2190_TideFree_SE_TP.grd).19

### The NOAA VDatum .gtx Format

To resolve the metadata ambiguities and Fortran padding artifacts inherent in the raw NGA .grd outputs, the broader geodetic software community widely utilizes the .gtx binary format. The .gtx format is famously employed by the U.S. National Oceanic and Atmospheric Administration (NOAA) VDatum tool and is natively supported by the OSGeo PROJ library for vertical coordinate transformations.21

The .gtx format abandons Fortran sequential markers and instead enforces a strict 40-byte binary metadata header followed immediately by a continuous, uninterrupted stream of float32 geoid undulation values.22 The byte layout of the 40-byte header is strictly ordered and defined by six specific spatial elements:

| **Byte Offset** | **Element** | **Data Type** | **Example Value (1-arcmin global)** | **Description** |
| --- | --- | --- | --- | --- |
| 0x00 | Lower-left Latitude | float64 (8 bytes) | -90.0 | Geodetic latitude of grid origin (South) |
| 0x08 | Lower-left Longitude | float64 (8 bytes) | 0.0 | Geodetic longitude of grid origin (West) |
| 0x10 | Delta Latitude | float64 (8 bytes) | 0.01666666666667 | Grid spacing in latitude (degrees) |
| 0x18 | Delta Longitude | float64 (8 bytes) | 0.01666666666667 | Grid spacing in longitude (degrees) |
| 0x20 | Number of Rows | int32 (4 bytes) | 10801 | Total latitudinal rows |
| 0x24 | Number of Columns | int32 (4 bytes) | 21600 | Total longitudinal columns |

Following this 40-byte header, the grid values are stored as raw float32 data in a row-major order. Crucially, the origin point of a .gtx file differs from the NGA raw format. The .gtx format begins at the lower-left corner (South-West origin, -90° Latitude, 0° Longitude) and proceeds eastward, moving row-by-row northward.22 Null, void, or missing values within a .gtx file are conventionally encoded precisely as -88.8888.22 While the endianness is generally Big-Endian by default convention, advanced parsers like PROJ handle both formats transparently by validating the latitude bounds in the header to auto-detect the byte order.23

### The GeographicLib Portable Gray Map (.pgm) Format

An immensely popular and highly optimized alternative to floating-point binaries is the .pgm format implemented by physicist Charles Karney in the ubiquitous GeographicLib C++ library.11 The Portable Gray Map (.pgm) is technically a standard image format, but GeographicLib repurposes it to store dense geoid grids.24

Instead of storing the undulations as 32-bit floats, GeographicLib quantizes the data and stores the grid as 16-bit unsigned integers (uint16). This design choice drastically reduces the file size on disk.11 A .pgm file begins with a plaintext ASCII header containing critical metadata, including the spatial bounds, the maximum bilinear and cubic interpolation errors, and the necessary mathematical scaling factors.25 After the newline character terminating the ASCII header, the raw 16-bit binary payload begins.

To reconstruct the floating-point geoid undulation from the compressed .pgm file, the 16-bit unsigned integer is subjected to a mathematical linear transformation defined in the ASCII header:

```
N = (raw_uint16 × scale) + offset
```

For the EGM2008 dataset, GeographicLib utilizes a predefined scale factor of 0.003 meters and an offset of -108 meters.17 By quantizing the continuous geopotential surface into 3-millimeter discrete steps, the entire 1-arcminute global grid fits into approximately 470 MB, compared to nearly 1 GB for the 32-bit raw .gtx or .grd formats.26 While this quantization does introduce an absolute maximum error of 1.5 millimeters into the height values, this artifact is mathematically negligible. GeographicLib's documentation explicitly notes that a 1.5-millimeter quantization noise is statistically smaller than the inherent errors introduced by linear spatial interpolation, rendering the compression highly advantageous for real-world applications.17

## Interpolation Methodologies and Accuracy Penalties

Because a geoid undulation grid provides elevation offset values at discrete, specific geographic coordinates, calculating the undulation at an arbitrary, non-integer latitude and longitude requires robust spatial interpolation. The mathematical approach chosen to interpolate the 1-arcminute EGM2008 grid dictates both the computational latency and the topographic accuracy of the resulting vertical transformation. The two primary methodologies utilized in geodetic software are Bilinear Interpolation and Bicubic Spline Interpolation.26

### Bilinear Interpolation

Bilinear interpolation is a first-order spatial technique that estimates the target value by taking a distance-weighted average of the four nearest neighboring grid nodes (forming a 2x2 matrix) enclosing the arbitrary query point.26 The mathematical weighting assigned to each node is inversely proportional to the Euclidean distance from the target point to that node.28

The primary advantage of bilinear interpolation is its sheer computational efficiency; it requires minimal memory lookups (only 4 data points) and performs a highly limited set of basic arithmetic multiplications. This makes it exceptionally suitable for resource-constrained environments, embedded flight systems, or real-time kinematic (RTK) field processors that require thousands of transformations per second.30

However, bilinear interpolation possesses a distinct mathematical limitation: while the resulting surface is continuous, it possesses discontinuous first derivatives (gradients) across the boundaries of the grid cells.26 This means that while the undulation height transitions smoothly, the slope of the geoid does not. When calculating precise gravitational vectors or high-resolution gravity anomalies, this sudden shift in the gradient across cell boundaries can introduce microscopic mathematical artifacting.26

### Bicubic Spline Interpolation

Bicubic interpolation represents a much higher-order approach, fitting a smooth cubic polynomial surface over the local geographic data.27 Rather than querying just 4 points, standard bicubic interpolation considers a 4x4 neighborhood of 16 grid nodes surrounding the query location.27 This higher-order method ensures that both the function values and their first derivatives are strictly continuous across all cell boundaries, resulting in an exceptionally smooth and mathematically rigorous surface representation that perfectly mimics the continuous nature of an equipotential gravity field.27

In highly optimized geodetic libraries such as GeographicLib, a modified bicubic approach is taken to improve processing performance and prevent polynomial overfitting. Instead of calculating a full 16-point matrix, GeographicLib employs a 12-point stencil—a 4x4 grid with the four extreme diagonal corners removed.26 This over-constrained least-squares fit, based on F. H. Lesh's algorithm for multidimensional polynomial curve fitting, effectively reduces quantization noise and suppresses Runge's phenomenon (the tendency for higher-order polynomials to exhibit unwanted oscillations at the fringes of the curve).26

### The Accuracy Penalty at 1-Arcminute Resolution

A central debate in geodetic software design is the accuracy penalty incurred by utilizing the faster bilinear interpolation instead of the mathematically superior bicubic spline.

If operating on the coarser 5-arcminute grid (~9 km spacing), the accuracy penalty is severe and unacceptable for precision surveying. On a 5-arcminute EGM2008 grid, bilinear interpolation yields an average error of 5 millimeters, but a worst-case penalty approaching 140 millimeters (14 cm) in regions with severe, high-frequency geoid gradients.26 Conversely, bicubic interpolation on that same 5-arcminute grid restricts the maximum error to just 3 millimeters.26

However, the dynamics shift radically when querying the dense 1-arcminute (~1.85 km) EGM2008 grid. The geoid is an equipotential surface, which physically acts as a low-pass filter of the Earth's topographic mass; it does not possess the jagged, discontinuous cliffs or sharp peaks found in visual Digital Elevation Models (DEMs).1 Because the 1-arcminute grid samples this naturally smooth gravitational surface so densely, the mathematical difference between linear and cubic approximations becomes incredibly minute.

| **Interpolation Method** | **Grid Resolution** | **Average Error** | **Maximum (Worst-Case) Error** |
| --- | --- | --- | --- |
| Bilinear | 5 arc-minutes | ~ 5 mm | 140 mm (14 cm) |
| Bicubic Spline | 5 arc-minutes | ~ 1 mm | 3 mm |
| Bilinear | 1 arc-minute | ~ 0.5 mm | 10 mm (1 cm) |
| Bicubic Spline | 1 arc-minute | ~ 0.5 mm | 2 mm |

As empirical evaluations demonstrate, at a 1-arcminute resolution, bilinear interpolation produces an average error of 0.5 millimeters, with a worst-case absolute penalty of only 10 millimeters (1 cm) globally.34 Bicubic interpolation on the same grid yields an identical average error of 0.5 millimeters and a worst-case error of 2 millimeters.34

Therefore, the absolute accuracy penalty for utilizing bilinear instead of bicubic interpolation on the 1-arcminute EGM2008 grid is no more than 8 millimeters under the most extreme global conditions.34 Given that the inherent commission and omission errors of the EGM2008 spherical harmonic model itself generally range from 5 to 10 centimeters globally 9, a sub-centimeter interpolation penalty is practically negligible. Consequently, bilinear interpolation is the recommended methodology for all but the most computationally unrestrained geodynamics applications, as it provides a nearly identical physical answer at a fraction of the computational cost.35

## Global Discrepancies Between EGM96 and EGM2008

The transition from EGM96 to EGM2008 resolved vast structural anomalies in the global gravity field. EGM96, truncated at a maximum degree and order of 360, suffered from extreme omission errors—meaning the model simply lacked the mathematical frequency bandwidth to represent short-wavelength, high-frequency gravitational features.36 EGM2008, expanding up to degree 2190, bridges this gap by integrating high-resolution GRACE satellite orbit tracking with dense terrestrial gravity databases and altimetry.10

### Magnitude and Location of Discrepancies

The difference in geoid undulation values between EGM96 and EGM2008 is not a simple, uniform datum shift; it is highly variable and deeply tied to the underlying topography of the Earth. Globally, the absolute discrepancies span a massive range. While the standard deviation between the two models generally hovers between 0.4 and 0.8 meters, local variances frequently exceed 1 to 5 meters.38

The largest discrepancies occur in regions characterized by severe topographic relief, complex coastal boundaries, and historically sparse terrestrial gravity sampling:

**Mountainous Terrain:** High-altitude mountain ranges exhibit the most violent short-wavelength gravity variations due to extreme crustal mass concentrations and isostatic compensation at the Moho boundary.41 Because of its low resolution, EGM96 heavily smoothed these anomalies, essentially failing to detect the gravitational signature of narrow mountain peaks. The introduction of Residual Terrain Modeling (RTM) and updated terrestrial gravity data in EGM2008 revealed highly localized discrepancies of several meters across the Andes in South America, the Himalayas, and the Rocky Mountains.39 For example, in the Andes, discrepancies exceeding 3 to 5 meters are commonly observed where EGM96 entirely missed high-frequency topographic signals.39

**Coastal and Littoral Zones:** The land-to-ocean transition presents immense geodetic challenges. EGM96 often contained significant artifacting in coastal zones because it struggled to seamlessly bridge satellite altimetry data (which is highly accurate over oceans) with terrestrial gravimetry (used over land).39 The lack of airborne gravity surveys meant that the immediate 20 to 30 kilometers off the coastline were poorly modeled.2 Recent analyses show that in extreme cases—such as the coastal regions of Argentina or steep oceanic trenches near Japan—the transition to EGM2008 corrected systemic biases of up to 5 meters, vastly improving the modeling of the mean dynamic topography and western boundary currents.39

**Polar Regions:** Areas like the Antarctic circumpolar region and Greenland continue to exhibit massive discrepancies. Historical deviations between EGM96 and modern geoids in Antarctica have ranged from 80 centimeters to extreme outliers of -213 centimeters.5 This is due to persistent limitations in satellite ground-track coverage near the poles, the sheer lack of terrestrial survey data across the ice sheets, and the physical complexities of modeling the gravitational pull of massive, shifting ice masses.5

### Impact on Aviation and Terrain Clearance

The magnitude of these discrepancies bears critical, life-safety implications for aviation, specifically regarding flight management systems, Terrain Awareness and Warning Systems (TAWS), Unmanned Aerial Vehicles (UAVs), and terrain clearance protocols.

Aircraft and UAV GNSS receivers calculate their three-dimensional position relative to the mathematical WGS 84 ellipsoid.14 However, to determine true altitude above Mean Sea Level (MSL)—which is strictly required to match barometric altimeters, publish approach plates, and clear physical terrain—the avionics software must subtract the local geoid undulation from the GNSS ellipsoidal height.7

If a flight computer or a TAWS database relies on the legacy EGM96 grid while navigating through a deep mountainous valley or executing a steep coastal approach, the omission error inherent in EGM96 could result in a calculated MSL altitude that is 2 to 5 meters higher or lower than physical reality.40 In modern aviation, particularly during Required Navigation Performance (RNP) instrument approaches or autonomous UAV low-level corridor flying, a 5-meter vertical error consumes a massive, unacceptable portion of the safety margin. By shifting to the EGM2008 high-resolution grid, the aviation industry gains a high-fidelity equipotential surface that tracks rugged terrain precisely, ensuring that GNSS-derived MSL altitudes correspond exactly to physical ground surveys and physical obstacles.49

## Geodetic Tide Systems and Copernicus DEM Compatibility

A highly nuanced, yet structurally vital, component of global geopotential modeling is the mathematical handling of permanent Earth tides. The gravitational pull of the Sun and the Moon causes a permanent, inelastic deformation of the Earth's crust and fluid oceans, shifting mass toward the equator. Geodesy defines three specific, mutually exclusive tide systems to address how this permanent deformation is treated within coordinate frames and gravity models 5:

- **Mean-Tide System**: This system retains the permanent tidal deformation in its entirety. The geoid surface includes the perpetual bulge caused by the celestial bodies, meaning the geoid matches the actual physical Mean Sea Level observed by coastal tide gauges.5

- **Zero-Tide System**: This system removes the direct, external gravitational attraction of the Sun and Moon, but explicitly *retains* the indirect effect (the physical, permanent elastic deformation of the Earth's crust itself).5 This is considered a middle-ground approach in physical geodesy.

- **Tide-Free (Non-Tidal) System**: This system completely removes both the direct attraction of the celestial bodies and the permanent elastic deformation of the Earth. It represents an entirely theoretical, mathematical Earth geometry that would exist if the Sun and Moon were suddenly removed from the solar system.5

### The EGM2008 Tide System and WGS 84 Alignment

The standard EGM2008 spherical harmonics and all pre-computed grid files distributed by the NGA are rigidly calculated and provided in the **Tide-Free** system.3

However, to ensure strict compatibility with the World Geodetic System 1984 (WGS 84), the NGA applies a highly specific mathematical constant to the Tide-Free EGM2008 grid. Because the native EGM2008 coefficients are referenced to an "ideal" mean-Earth ellipsoid, applying a zero-degree term of exactly -41 centimeters shifts the entire global undulation grid so that it geometrically aligns with the defined WGS 84 reference ellipsoid.3 This constant -41 cm shift is intrinsically baked into the standard 1-arcminute and 2.5-arcminute grid files available for public download, seamlessly converting theoretical Tide-Free undulations into WGS 84-compatible offsets.3

### Interaction with the Copernicus DEM

The Copernicus Digital Elevation Model (Copernicus DEM or GLO-30), derived from the European Space Agency's TanDEM-X synthetic aperture radar mission, is currently widely regarded as the premier open-source global elevation dataset.54

According to the official specifications, the orthometric heights in the Copernicus DEM are explicitly referenced to the EGM2008 geoid.56 Consequently, because the EGM2008 reference surface is Tide-Free, the orthometric heights provided by the Copernicus DEM are inherently aligned with the Tide-Free system.57

While this internal consistency is mathematically sound for satellite operations, it presents a significant obstacle for certain hydrometeorological, flood modeling, and oceanographic analyses. Local tidal gauges, national vertical datums, and maritime bathymetry charts inherently operate in the real, physical world, which maps strictly to the Mean-Tide system.57 Comparing Copernicus DEM terrain elevations directly against local coastal sea-level data (for tasks like tsunami inundation or sea-level rise modeling) will yield a systematic vertical bias if the tide systems are not mathematically reconciled.42

If a correction is needed to convert the Tide-Free geoid undulation (inherent in Copernicus DEM) to a physical Mean-Tide geoid undulation, the mathematical offset is defined by the following equation:

```
ΔN_tide = (1 + k) × 0.198 × (3/2 × sin²φ - 1/2)   [meters]

N_mean = N_tidefree + ΔN_tide
```

Where φ is the geodetic latitude and k is the Love number (typically accepted as k ≈ 0.3).5 In practice, this means the difference between a Tide-Free Copernicus DEM elevation and a Mean-Tide local elevation can vary smoothly by approximately 10 to 12 centimeters depending on the latitude—an offset that must be actively managed when integrating datasets.5

## Open-Source Implementations for EGM2008 Grid Lookup

The global geodetic community relies on a robust ecosystem of open-source libraries to ingest these grids and perform spatial lookups. Analyzing the architecture of these libraries reveals distinctly different paradigms regarding grid loading, memory management, and computational design.

### The C++ GeographicLib Implementation

Developed by Charles Karney, the C++ GeographicLib suite is universally considered the gold standard for geoid evaluations.59 As previously discussed, it expects the grid in its custom 16-bit .pgm format to aggressively save disk space.24

To handle the grid loading, GeographicLib explicitly avoids recklessly dumping the entire 470 MB file into RAM. Instead, it utilizes highly sophisticated memory-mapped file access and an LRU (Least Recently Used) caching mechanism.25 The system reads the .pgm file in discrete chunks, holding only a minimal matrix of 16-bit integers (such as a 3x3 or 4x4 block) in memory at any given moment. During an interpolation query, it fetches the required nodes, applies the scale and offset to convert them to double precision floats on the fly, and then executes its signature 12-point bicubic spline algorithm.25 This intelligent architecture guarantees millimeter-level accuracy while maintaining a nearly invisible memory footprint, making it ideal for desktop GIS software, mobile devices, and heavy bulk-processing tasks where RAM is limited.26

### The Python pygeodesy Library

The pygeodesy module provides a comprehensive, pure Python transcoding of Karney's algorithms, exposing classes like GeoidKarney and GeoidPGM.60

While it fully supports both bilinear and bicubic interpolation (controllable via the kind parameter), its internal memory architecture diverges sharply from the C++ implementation. When loading a .pgm file using GeoidPGM, the library relies heavily on the underlying SciPy package (specifically scipy.interpolate functions like RectBivariateSpline).61 SciPy algorithms fundamentally require the data to be loaded contiguously into memory as 8-byte float64 NumPy arrays.61

Consequently, the moment the 1-arcminute .pgm grid is ingested, pygeodesy unpacks all 233 million 16-bit integers, applies the mathematical offsets, and instantiates a massive 64-bit float array.61 This behavior causes a catastrophic explosion in memory footprint, generating a RAM utilization four times larger than the physical .pgm file size.61 To prevent Out-Of-Memory (OOM) crashes—particularly on 32-bit Python deployments or cloud lambdas—the library provides a mandatory crop argument. This enables developers to load only a specific bounding box of the global grid into the SciPy interpolator. For example, cropping to the contiguous United States limits the memory load to roughly 3% of the global dataset, making the application viable.61

### The Rust egm2008 Crate

The Rust ecosystem tackles the geoid lookup problem from an entirely different angle. The egm2008 crate serves as a lightweight, memory-safe alternative intended for high-concurrency or headless compute engines (such as the backend engines for aerial survey flight planning tools).

Rather than parsing .pgm, .grd, or .gtx files at runtime, the crate's architecture relies on a specialized compile-time build script (generate.py / build.rs).47 The build script requires the host system to have gfortran installed. During the build process, it downloads the original NGA data files, compiles the official NGA Fortran interpolation program (hsynth_WGS84.f), and uses it to automatically generate a massive geoid.rs source code file containing the grid data as static arrays.47

By rendering the interpolation data directly into compiled Rust arrays, the crate sidesteps the need for runtime file I/O operations and memory-mapped disk access entirely. The grid data is embedded directly into the application binary, enabling incredibly fast, memory-safe bilinear or polynomial lookups directly from the application's memory space. This design explicitly sacrifices binary file size (the resulting executable becomes massive) in favor of supreme execution speed and the absolute elimination of runtime dependency errors, fulfilling the strict memory-safety and high-concurrency paradigms required by modern Rust backend development.

## Memory Footprint of the 1-Arcminute Grid

Understanding the exact memory footprint of the 1-arcminute global EGM2008 grid is critical for systems programming, particularly when architecting backend GIS servers and deciding between single-precision (f32) and double-precision (f64) floating-point storage limits.

The 1-arcminute global grid encompasses 10,801 rows and 21,600 columns.17 The total number of discrete grid nodes is calculated by multiplying the latitudinal and longitudinal dimensions:

```
10,801 × 21,600 = 233,301,600 grid nodes
```

When this dataset is loaded entirely into Random Access Memory (RAM), the footprint scales strictly with the selected datatype. The following table illustrates the memory requirements for loading the entire 1-arcminute grid into RAM without chunking or cropping:

| **Data Type** | **Bytes per Node** | **Total Bytes** | **Memory Footprint in RAM** | **Use Case Example** |
| --- | --- | --- | --- | --- |
| uint16 | 2 Bytes | 466,603,200 Bytes | ~445 MiB / 466 MB | GeographicLib .pgm (Disk) |
| f32 | 4 Bytes | 933,206,400 Bytes | ~890 MiB / 933 MB | Raw NGA .grd or PROJ .gtx |
| f64 | 8 Bytes | 1,866,412,800 Bytes | ~1.74 GiB / 1.86 GB | PyGeodesy / SciPy Arrays |

As illustrated, loading the raw grid as 32-bit floats (f32) demands nearly 1 GB of contiguous memory. If the data is unpacked into 64-bit floats (f64) to accommodate advanced spline calculations or NumPy ecosystem requirements (as seen in the Python pygeodesy architecture), the memory footprint balloons to nearly 1.9 Gigabytes.61

For modern desktop workstations, dedicating 1.9 GB of RAM to a single geoid lookup matrix is generally acceptable. However, for headless compute servers processing thousands of concurrent geospatial queries, or for embedded avionics systems on UAVs with strict memory limits, a 1.9 GB memory allocation is entirely unfeasible. In these highly constrained environments, developers must either rely on the intelligent chunked caching strategy of GeographicLib (which caps the active footprint at mere megabytes), physically crop the dataset at load time, or accept the ~933 MB penalty of a static f32 array.25

## Conclusion

The Earth Gravitational Model 2008 remains an extraordinary achievement in physical geodesy, providing the required high-frequency spectral resolution to bridge the gap between global mathematical models and localized topographic reality.

The technical handling of the EGM2008 pre-computed grids requires rigorous attention to software architecture. Parsing the original NGA files requires stripping unformatted Fortran record padding markers, while utilizing the modern .gtx format requires strict adherence to its 40-byte spatial header to avoid misalignment. Furthermore, the mathematical analysis dictates that for the dense 1-arcminute grid, the accuracy penalty of utilizing rapid bilinear interpolation over computationally heavy bicubic splines is statistically negligible (yielding a maximum worst-case penalty of ~8 millimeters). This renders bilinear interpolation vastly superior for real-time RTK and UAV performance.

The shift from the legacy EGM96 to EGM2008 eliminates massive omissions errors spanning 1 to 5 meters in complex coastal and mountainous regions, establishing EGM2008 as a critical upgrade for aviation terrain clearance and autonomous navigation. However, when fusing this global model with modern topographic data like the Copernicus DEM, engineers must remain keenly aware of geodetic tide systems. Because both the EGM2008 coefficients and the dependent Copernicus DEM are mapped exclusively in a Tide-Free system, strict mathematical conversions are required when interfacing with local, Mean-Tide oceanographic data to avoid systemic biases.

Ultimately, the optimal implementation strategy for EGM2008 grid lookup is highly domain-specific. While Python's pygeodesy offers rapid prototyping at the cost of intense RAM bloat, and Rust's egm2008 crate guarantees memory safety via ahead-of-time static compilation, the C++ GeographicLib framework remains the premier programmatic solution. By leveraging 16-bit quantization, memory mapping, and a mathematically refined 12-point cubic stencil, it perfectly balances absolute mathematical precision with supreme computational efficiency.

#### Works cited

- earth map control: Topics by Science.gov, accessed March 23, 2026, [https://www.science.gov/topicpages/e/earth+map+control](https://www.science.gov/topicpages/e/earth+map+control)

- global geoid models: Topics by Science.gov, accessed March 23, 2026, [https://www.science.gov/topicpages/g/global+geoid+models](https://www.science.gov/topicpages/g/global+geoid+models)

- EGM 2008 Worldwide Data 2.5 x 2.5 Minute from NGA - Overview - ArcGIS Online, accessed March 23, 2026, [https://www.arcgis.com/home/item.html?id=92c70b63f9804b63b889495c23258129](https://www.arcgis.com/home/item.html?id=92c70b63f9804b63b889495c23258129)

- ADAPTATION OF THE GLOBAL GEOID MODEL EGM2008 ON CAMPANIA REGION (ITALY) BASED ON GEODETIC NETWORK POINTS, accessed March 23, 2026, [https://d-nb.info/1243172371/34](https://d-nb.info/1243172371/34)

- 11. the egm96 geoid undulation with respect to the wgs84 ellipsoid - cddis - NASA, accessed March 23, 2026, [https://cddis.nasa.gov/926/egm96/doc/S11.HTML](https://cddis.nasa.gov/926/egm96/doc/S11.HTML)

- (PDF) ADAPTATION OF THE GLOBAL GEOID MODEL EGM2008 ON CAMPANIA REGION (ITALY) BASED ON GEODETIC NETWORK POINTS - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/355137098_ADAPTATION_OF_THE_GLOBAL_GEOID_MODEL_EGM2008_ON_CAMPANIA_REGION_ITALY_BASED_ON_GEODETIC_NETWORK_POINTS](https://www.researchgate.net/publication/355137098_ADAPTATION_OF_THE_GLOBAL_GEOID_MODEL_EGM2008_ON_CAMPANIA_REGION_ITALY_BASED_ON_GEODETIC_NETWORK_POINTS)

- An Assessment of Geoidal Undulations Determined From GPS/LEVELLING Method and EGM2008 - IOSR Journal, accessed March 23, 2026, [https://www.iosrjournals.org/iosr-jestft/papers/Vol16-Issue5/Ser-2/A1605020112.pdf](https://www.iosrjournals.org/iosr-jestft/papers/Vol16-Issue5/Ser-2/A1605020112.pdf)

- Session 2.2: Gravity and WHS - UNOOSA, accessed March 23, 2026, [https://www.unoosa.org/pdf/icg/2012/7.pdf](https://www.unoosa.org/pdf/icg/2012/7.pdf)

- Determination local geoid Heights Using RTK-DGPS/Leveling and transformation methods, accessed March 23, 2026, [https://ijs.uobaghdad.edu.iq/index.php/eijs/article/download/7237/2520/62816](https://ijs.uobaghdad.edu.iq/index.php/eijs/article/download/7237/2520/62816)

- Geoid Determination Based on a Combination of Terrestrial and Airborne Gravity Data in South Korea - School of Earth Sciences - The Ohio State University, accessed March 23, 2026, [https://earthsciences.osu.edu/sites/earthsciences.osu.edu/files/report-507.pdf](https://earthsciences.osu.edu/sites/earthsciences.osu.edu/files/report-507.pdf)

- Earth Gravitational Model - Wikipedia, accessed March 23, 2026, [https://en.wikipedia.org/wiki/Earth_Gravitational_Model](https://en.wikipedia.org/wiki/Earth_Gravitational_Model)

- The Development and Evaluation of the Earth Gravitational Model 2008 (EGM2008) (vol 117, B04406, 2012) | Request PDF - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/232095456_The_Development_and_Evaluation_of_the_Earth_Gravitational_Model_2008_EGM2008_vol_117_B04406_2012](https://www.researchgate.net/publication/232095456_The_Development_and_Evaluation_of_the_Earth_Gravitational_Model_2008_EGM2008_vol_117_B04406_2012)

- Establishment of a Geometric Geoid Model and Evaluation of the EGM2008 and EIGEN-6CA Models over the Dakar-Thies-Mbour Triangle in Senegal - SCIRP, accessed March 23, 2026, [https://www.scirp.org/journal/paperinformation?paperid=137677](https://www.scirp.org/journal/paperinformation?paperid=137677)

- Guidelines for Implementation of EGM-08 elevation model for processing of NOC application in NOCAS - Vertical (height) Reference System, accessed March 23, 2026, [https://nocas2.aai.aero/nocas/AAI_Links/Guidelines%20for%20Vertical%20Reference%20System%20EGM08.pdf](https://nocas2.aai.aero/nocas/AAI_Links/Guidelines%20for%20Vertical%20Reference%20System%20EGM08.pdf)

- EGM2008 Geoid Model for WGS 84 | PDF - Scribd, accessed March 23, 2026, [https://www.scribd.com/document/270641397/Egm2008-Wgs-84](https://www.scribd.com/document/270641397/Egm2008-Wgs-84)

- The development and evaluation of the Earth Gravitational Model 2008 (EGM2008) - Geofisica.cl, accessed March 23, 2026, [http://www.geofisica.cl/usach/egm2008.pdf](http://www.geofisica.cl/usach/egm2008.pdf)

- Add NGA EGM 2008 1x1 grid_alternative entry · Issue #111 · OSGeo/PROJ-data - GitHub, accessed March 23, 2026, [https://github.com/OSGeo/PROJ-data/issues/111](https://github.com/OSGeo/PROJ-data/issues/111)

- Newest 'geoid' Questions - Geographic Information Systems Stack Exchange, accessed March 23, 2026, [https://gis.stackexchange.com/questions/tagged/geoid?tab=Newest](https://gis.stackexchange.com/questions/tagged/geoid?tab=Newest)

- World Geodetic System 1984 (WGS 84) - NGA - Office of Geomatics, accessed March 23, 2026, [https://earth-info.nga.mil/GandG/wgs84/index.html](https://earth-info.nga.mil/GandG/wgs84/index.html)

- NGA - Office of Geomatics, accessed March 23, 2026, [https://earth-info.nga.mil/](https://earth-info.nga.mil/)

- The GGXF Standard File Format for Gridded Geodetic Data - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/382610390_The_GGXF_Standard_File_Format_for_Gridded_Geodetic_Data](https://www.researchgate.net/publication/382610390_The_GGXF_Standard_File_Format_for_Gridded_Geodetic_Data)

- NOAA/NOS's VDatum:Interpolation and Transformation Grid Format, accessed March 23, 2026, [https://vdatum.noaa.gov/docs/gtx_info.html](https://vdatum.noaa.gov/docs/gtx_info.html)

- Geodetic TIFF grids (GTG) — PROJ 9.8.0 documentation, accessed March 23, 2026, [https://proj.org/en/stable/specifications/geodetictiffgrids.html](https://proj.org/en/stable/specifications/geodetictiffgrids.html)

- [geographiclib] 01/03: Imported Upstream version 1.34 - Debian, accessed March 23, 2026, [https://alioth-lists.debian.net/pipermail/pkg-grass-devel/2014-March/018641.html](https://alioth-lists.debian.net/pipermail/pkg-grass-devel/2014-March/018641.html)

- Geoid height - GeographicLib, accessed March 23, 2026, [http://lira.no-ip.org:8080/doc/geographiclib/html/geoid.html](http://lira.no-ip.org:8080/doc/geographiclib/html/geoid.html)

- GeographicLib.dox - GitLab, accessed March 23, 2026, [https://git.geomar.de/open-source/dsm-sonar-software/-/blob/6bd3cba67bd61a41ab2e41ddbc3590352a2ec9ea/ExternalLibs/GeographicLib-1.34/doc/GeographicLib.dox](https://git.geomar.de/open-source/dsm-sonar-software/-/blob/6bd3cba67bd61a41ab2e41ddbc3590352a2ec9ea/ExternalLibs/GeographicLib-1.34/doc/GeographicLib.dox)

- Bicubic interpolation - Wikipedia, accessed March 23, 2026, [https://en.wikipedia.org/wiki/Bicubic_interpolation](https://en.wikipedia.org/wiki/Bicubic_interpolation)

- What are the differences between bilinear and bicubic interpolation in CNN upsampling?, accessed March 23, 2026, [https://massedcompute.com/faq-answers/?question=What%20are%20the%20differences%20between%20bilinear%20and%20bicubic%20interpolation%20in%20CNN%20upsampling?](https://massedcompute.com/faq-answers/?question=What+are+the+differences+between+bilinear+and+bicubic+interpolation+in+CNN+upsampling?)

- Image Upscaling using Bicubic Interpolation | by Amanrao - Medium, accessed March 23, 2026, [https://medium.com/@amanrao032/image-upscaling-using-bicubic-interpolation-ddb37295df0](https://medium.com/@amanrao032/image-upscaling-using-bicubic-interpolation-ddb37295df0)

- BiLinear or BiCubic file interpolation. - WCB, accessed March 23, 2026, [https://www.wcb5.com/post/bilinear-or-bicubic-file-interpolation](https://www.wcb5.com/post/bilinear-or-bicubic-file-interpolation)

- (PDF) Low-Cost Implementation of Bilinear and Bicubic Image Interpolation for Real-Time Image Super-Resolution - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/344334296_Low-Cost_Implementation_of_Bilinear_and_Bicubic_Image_Interpolation_for_Real-Time_Image_Super-Resolution](https://www.researchgate.net/publication/344334296_Low-Cost_Implementation_of_Bilinear_and_Bicubic_Image_Interpolation_for_Real-Time_Image_Super-Resolution)

- Bicubic Interpolation: Professional Quality Image Scaling, accessed March 23, 2026, [https://image-scaler.com/blog/bicubic-interpolation/](https://image-scaler.com/blog/bicubic-interpolation/)

- ERRORS MEASUREMENT OF INTERPOLATION METHODS FOR GEOID MODELS: STUDY CASE IN THE BRAZILIAN REGION - SciELO, accessed March 23, 2026, [https://www.scielo.br/j/bcg/a/sgwx95bwgD8rRGw8LfFsV8B/?format=pdf&lang=en](https://www.scielo.br/j/bcg/a/sgwx95bwgD8rRGw8LfFsV8B/?format=pdf&lang=en)

- Geoid reading object (sarpy.io.DEM.geoid) - sarpy's documentation! - Read the Docs, accessed March 23, 2026, [https://sarpy.readthedocs.io/en/latest/io/DEM/geoid.html](https://sarpy.readthedocs.io/en/latest/io/DEM/geoid.html)

- (PDF) Errors measurement of interpolation methods for geoid models: Study case in the Brazilian region - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/324070030_Errors_measurement_of_interpolation_methods_for_geoid_models_Study_case_in_the_Brazilian_region](https://www.researchgate.net/publication/324070030_Errors_measurement_of_interpolation_methods_for_geoid_models_Study_case_in_the_Brazilian_region)

- Assessment of EGM2008 over Germany Using Accurate Quasigeoid Heights from Vertical Deflections, GCG05 and GPS/levelling - geodaesie.info, accessed March 23, 2026, [https://geodaesie.info/images/zfv/136-jahrgang-2011/downloads/zfv_2011_3_Hirt.pdf](https://geodaesie.info/images/zfv/136-jahrgang-2011/downloads/zfv_2011_3_Hirt.pdf)

- Evaluation of EGM08 - globally, and locally in South Korea - ISG, accessed March 23, 2026, [https://www.isgeoid.polimi.it/Newton/Newton_4/Report_G4_Jekelietal.pdf](https://www.isgeoid.polimi.it/Newton/Newton_4/Report_G4_Jekelietal.pdf)

- Ability of the EGM2008 High Degree Geopotential Model to Calculate a Local Geoid Model in Valencia, Eastern Spain | Request PDF - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/226461135_Ability_of_the_EGM2008_High_Degree_Geopotential_Model_to_Calculate_a_Local_Geoid_Model_in_Valencia_Eastern_Spain](https://www.researchgate.net/publication/226461135_Ability_of_the_EGM2008_High_Degree_Geopotential_Model_to_Calculate_a_Local_Geoid_Model_in_Valencia_Eastern_Spain)

- Newton's Bulletin - ISG, accessed March 23, 2026, [https://www.isgeoid.polimi.it/Newton/Newton_4/NEWTON4_TOTAL.pdf](https://www.isgeoid.polimi.it/Newton/Newton_4/NEWTON4_TOTAL.pdf)

- Simplifying EGM96 undulation calculation in ROPP - ROM SAF, accessed March 23, 2026, [https://rom-saf.eumetsat.int/general-documents/rsr/rsr_16.pdf](https://rom-saf.eumetsat.int/general-documents/rsr/rsr_16.pdf)

- Integrated Geophysical Analysis of Passive Continental Margins Insights into the Crustal Structure of the Namibian Margin from Magnetotelluric, Gravity, and Seismic Data - OceanRep, accessed March 23, 2026, [https://oceanrep.geomar.de/59349/1/Franz_Thesis_20230404_PublishOnline.pdf](https://oceanrep.geomar.de/59349/1/Franz_Thesis_20230404_PublishOnline.pdf)

- Height biases of SRTM DEM related to EGM96: from a global perspective to regional practice | Request PDF - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/306390307_Height_biases_of_SRTM_DEM_related_to_EGM96_from_a_global_perspective_to_regional_practice](https://www.researchgate.net/publication/306390307_Height_biases_of_SRTM_DEM_related_to_EGM96_from_a_global_perspective_to_regional_practice)

- VALIDATION OF EGM2008 OVER ARGENTINA - ISG, accessed March 23, 2026, [https://www.isgeoid.polimi.it/Newton/Newton_4/Report_A4_Argentina.pdf](https://www.isgeoid.polimi.it/Newton/Newton_4/Report_A4_Argentina.pdf)

- Evolution of geoids in recent years and its impact on oceanography - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/263723227_Evolution_of_geoids_in_recent_years_and_its_impact_on_oceanography](https://www.researchgate.net/publication/263723227_Evolution_of_geoids_in_recent_years_and_its_impact_on_oceanography)

- Geoid differences between regional gravimetric geoid models and... | Download Scientific Diagram - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/figure/Geoid-differences-between-regional-gravimetric-geoid-models-and-GPS-leveling-geoid_fig1_253700493](https://www.researchgate.net/figure/Geoid-differences-between-regional-gravimetric-geoid-models-and-GPS-leveling-geoid_fig1_253700493)

- EVALUATION OF EGM2008 BY MEANS OF GPS/LEVELLING IN UGANDA BY - AAU-ETD - Addis Ababa University, accessed March 23, 2026, [https://etd.aau.edu.et/bitstreams/bbddf4c9-eacb-4be6-a7b7-2e38f7fd771c/download](https://etd.aau.edu.et/bitstreams/bbddf4c9-eacb-4be6-a7b7-2e38f7fd771c/download)

- flightaware/egm2008: Earth Gravitational Model (EGM2008) - GitHub, accessed March 23, 2026, [https://github.com/flightaware/egm2008](https://github.com/flightaware/egm2008)

- 1 Differences between EGM2008 and EGM96 geoid heights - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/figure/Differences-between-EGM2008-and-EGM96-geoid-heights_fig3_227261263](https://www.researchgate.net/figure/Differences-between-EGM2008-and-EGM96-geoid-heights_fig3_227261263)

- Third International Conference on Remote Sensing, Mapping, and Geographic Information Systems (RSMG 2025) - SPIE, accessed March 23, 2026, [https://spie.org/Publications/Proceedings/Volume/13791](https://spie.org/Publications/Proceedings/Volume/13791)

- Evaluation of EGM 2008 with EGM96 and its Utilization in Topographical Mapping Projects, accessed March 23, 2026, [https://www.researchgate.net/publication/251370782_Evaluation_of_EGM_2008_with_EGM96_and_its_Utilization_in_Topographical_Mapping_Projects](https://www.researchgate.net/publication/251370782_Evaluation_of_EGM_2008_with_EGM96_and_its_Utilization_in_Topographical_Mapping_Projects)

- The permanent tide and the International Height Reference Frame IHRF, accessed March 23, 2026, [https://d-nb.info/1248699106/34](https://d-nb.info/1248699106/34)

- 1 General definitions and numerical standards (16 November 2017) - IERS Conventions Centre, accessed March 23, 2026, [https://iers-conventions.obspm.fr/content/chapter1/icc1.pdf](https://iers-conventions.obspm.fr/content/chapter1/icc1.pdf)

- Evaluation of the EGM2008 Gravity Field by Means of GPS- Levelling and Sea Surface Topography Solutions - ISG, accessed March 23, 2026, [https://www.isgeoid.polimi.it/Newton/Newton_4/Report_G1_Gruber.pdf](https://www.isgeoid.polimi.it/Newton/Newton_4/Report_G1_Gruber.pdf)

- Error-reduced digital elevation models and high-resolution land cover roughness in mapping tsunami exposure for low elevation co - electronic library -, accessed March 23, 2026, [https://elib.dlr.de/217983/1/1-s2.0-S2352938524003021-main.pdf](https://elib.dlr.de/217983/1/1-s2.0-S2352938524003021-main.pdf)

- The Earth Topography 2022 (ETOPO 2022) global DEM dataset - ESSD Copernicus, accessed March 23, 2026, [https://essd.copernicus.org/articles/17/1835/2025/essd-17-1835-2025.pdf](https://essd.copernicus.org/articles/17/1835/2025/essd-17-1835-2025.pdf)

- Climate change - Planning Inspectorate, accessed March 23, 2026, [https://nsip-documents.planninginspectorate.gov.uk/published-documents/EN020026-001034-Alan%20George%20Jones.pdf](https://nsip-documents.planninginspectorate.gov.uk/published-documents/EN020026-001034-Alan%20George%20Jones.pdf)

- Geoid Studies in Two Test Areas in Greece Using Different Geopotential Models towards the Estimation of a Reference Geopotential Value - MDPI, accessed March 23, 2026, [https://www.mdpi.com/2072-4292/15/17/4282](https://www.mdpi.com/2072-4292/15/17/4282)

- Regional modeling of the impacts of tidal flooding in the context of mean sea level rise on low-lying in the Global South - EGUsphere, accessed March 23, 2026, [https://egusphere.copernicus.org/preprints/2026/egusphere-2025-4929/egusphere-2025-4929.pdf](https://egusphere.copernicus.org/preprints/2026/egusphere-2025-4929/egusphere-2025-4929.pdf)

- GeographicLib — GeographicLib 2.5 documentation, accessed March 23, 2026, [https://geographiclib.sourceforge.io/](https://geographiclib.sourceforge.io/)

- pygeodesy.geoids, accessed March 23, 2026, [https://mrjean1.github.io/PyGeodesy/docs/pygeodesy.geoids-module.html](https://mrjean1.github.io/PyGeodesy/docs/pygeodesy.geoids-module.html)

- pygeodesy.geoids.GeoidPGM, accessed March 23, 2026, [https://mrjean1.github.io/PyGeodesy/docs/pygeodesy.geoids.GeoidPGM-class.html](https://mrjean1.github.io/PyGeodesy/docs/pygeodesy.geoids.GeoidPGM-class.html)

- PyGeodesy/pygeodesy/geoids.py at master · mrJean1/PyGeodesy - GitHub, accessed March 23, 2026, [https://github.com/mrJean1/PyGeodesy/blob/master/pygeodesy/geoids.py](https://github.com/mrJean1/PyGeodesy/blob/master/pygeodesy/geoids.py)

- egm2008 - crates.io: Rust Package Registry, accessed March 23, 2026, [https://crates.io/crates/egm2008](https://crates.io/crates/egm2008)