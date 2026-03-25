# Mathematical Analysis of Flight Planning for Multi-Camera Oblique Aerial Photogrammetry

## Introduction to the Geometrical Shift in Airborne Photogrammetry

The transition from traditional two-dimensional orthogonal mapping to high-fidelity, three-dimensional digital twin generation has necessitated a fundamental paradigm shift in the mathematics and operational execution of airborne photogrammetry. Conventional nadir imagery, while mathematically optimal for generating two-dimensional orthomosaics and two-and-a-half-dimensional Digital Terrain Models (DTMs), suffers from severe geometric limitations when tasked with reconstructing the vertical surfaces of complex urban environments. To resolve building lean occlusions, eliminate data voids in narrow urban canyons, and capture high-resolution facade textures, multi-camera oblique photogrammetry systems have become the prevailing industry standard. These systems, frequently deploying a "Maltese-cross" or penta-camera configuration consisting of one central nadir sensor and four oblique sensors oriented toward the cardinal compass directions, introduce unprecedented complexities into the photogrammetric pipeline.

The integration of oblique viewing angles fundamentally alters the mathematical framework of flight planning, sensor orientation, and block adjustment. Because oblique optical axes intersect the terrain at steep angles, the resulting ground footprints are highly distorted trapezoids rather than regular rectangles. This geometric distortion induces severe scale variations and non-uniform Ground Sample Distances (GSD) across the image plane, complicating every subsequent step of the photogrammetric workflow. Consequently, the standard photogrammetric assumptions governing flight line spacing, forward overlap, sidelap, and automated tie-point matching must be entirely reformulated to account for extreme perspective distortions and affine geometric transformations.

This comprehensive report provides an exhaustive mathematical analysis of flight planning and image geometry specifically tailored for multi-camera oblique photogrammetry systems. Drawing upon empirical findings from the ISPRS/EuroSDR multi-platform photogrammetry benchmarks, alongside published optical and mechanical specifications from industry-leading manufacturers such as Leica Geosystems and Phase One, this analysis systematically evaluates oblique footprint geometry, collinearity equations incorporating fixed mount angles, and flight line optimization algorithms. Furthermore, it details the profound implications of oblique geometries on Bundle Block Adjustment (BBA), dense image matching, and the derivation of highly accurate three-dimensional urban models.

## Oblique Camera Geometry and Ground Sample Distance Variation

### Projective Geometry and the Trapezoidal Footprint Derivation

In traditional nadir photogrammetry, assuming a relatively flat terrain model, the intersection of the camera's field of view with the ground plane yields a rectangular footprint, allowing for uniform scaling and straightforward overlap calculations. In oblique photogrammetry, the camera's optical axis is intentionally tilted by an angle  (or ) from the vertical nadir line. For optimal urban mapping applications, this tilt angle typically ranges between 40° and 45°.1 This intentional deviation causes the planar image array to intersect the horizontal ground plane at a severe, non-orthogonal angle, fundamentally altering the projective geometry and casting a trapezoidal footprint onto the terrain.4

The precise geometry of this projection is defined by the principal plane, which is the vertical plane containing both the camera's optical axis and the vertical plumb line passing through the perspective center .6 The principal plane intersects the physical image plane along a line known as the principal line. Within this geometric construct, several critical nodes define the mathematical behavior of the oblique projection:

- The principal point P, defined as the exact coordinate where the optical axis orthogonally intersects the image plane.

- The image nadir point N, defined as the intersection of the vertical plumb line from the perspective center O with the image plane.

- The isocenter , located directly on the principal line exactly halfway between the principal point  and the nadir point . The isocenter represents the unique point from which radial angles measured on the image plane are mathematically true to their corresponding angles measured on the ground plane.6

- The horizon point H, defined as the intersection of a horizontal plane passing through the perspective center  with the tilted image plane.

The spatial distances between these critical points on the two-dimensional image plane are strictly governed by the camera constant (the calibrated focal length, denoted as  or ) and the mechanical tilt angle . The distance from the principal point to the image nadir point is analytically derived as:

 6

Because the angle of depression  is the geometric complement of the tilt angle (such that ), the distance to the horizon point  is mathematically expressed using trigonometric substitution:

 6

To compute the absolute ground coordinates of the trapezoidal footprint's vertices, the inverse collinearity equations must be applied to the four extreme corners of the camera's complementary metal-oxide-semiconductor (CMOS) sensor array. For a sensor array of physical width S_w and physical height S_h, the image corners located at (±S_w/2, ±S_h/2) are projected through the perspective center down to the ground elevation reference plane, typically defined as Z = Z_ground. The resulting geometric expansion dictates that the near edge of the footprint—the edge closest to the aircraft's nadir position—is significantly narrower in physical ground width than the far edge, which extends outward toward the true horizon.5

### The Mathematical Variation of Ground Sample Distance

Because the slant range from the perspective center to the ground surface increases drastically and non-linearly from the near edge to the far edge of the oblique footprint, the Ground Sample Distance (GSD) degrades proportionally. In nadir acquisitions over flat terrain, the theoretical GSD is effectively uniform and is calculated via a simple linear ratio:

 4

where  represents the flying height above ground level (AGL),  represents the physical pixel pitch on the sensor (often measured in micrometers), and  represents the focal length of the lens.4

For an oblique camera tilted at an angle τ relative to the vertical axis, the slant distance D_slant along the principal optical axis is defined geometrically as D_slant = H_agl / cos(τ). Consequently, the localized GSD precisely at the principal point is calculated as:

```
GSD_oblique = (D_slant × pixel_pitch) / f = (H_agl × pixel_pitch) / (f × cos(τ))

Equivalently: GSD_oblique = GSD_nadir / cos(τ) = GSD_nadir × sec(τ)
```

At τ = 45°: GSD_oblique = GSD_nadir × √2 ≈ 1.414 × GSD_nadir

 10

However, unlike a nadir image, the GSD in an oblique image is highly anisotropic; it is not uniform across the transverse and longitudinal axes. Along the vertical axis of the image, which corresponds to the depth of the footprint stretching toward the horizon, the ground scale degrades at a highly accelerated rate due to the compounded effects of increasing distance and the shallow incident viewing angle.

The rigorous analytical derivation of the scale numbers  requires evaluating the partial derivatives of the ground coordinates with respect to the corresponding image coordinates. According to established photogrammetric homography mappings, the scale number  representing the across-track resolution (parallel to the horizon) and the scale number  representing the along-track depth resolution differ fundamentally.10 Using the homography matrix  that maps the image plane to the ground plane , the differentials for the lengths of the infinitesimal image segments yield:

 10  10

At the extreme far edge of the oblique image frame, the incidence angle approaches the horizon line. This causes the projected pixel footprints to stretch into highly elongated, distorted ellipses on the ground surface. The theoretical GSD at this far boundary is vastly larger than the GSD at the principal point centroid; in extreme high-oblique cases where the field of view inadvertently encompasses the horizon, the far-edge GSD mathematically diverges toward infinity.8 To account for this severe degradation in operational flight planning, the nominal flying height  is frequently adjusted by the cosine of the depression angle to derive a more realistic theoretical boundary for the orthophoto generation.8

### Standard Formulations for the Mean GSD

Because representing an entire oblique image by a single GSD value is inherently inaccurate and mathematically flawed, modern flight planning software and photogrammetric standards—such as those adopted by the ISPRS and EuroSDR multi-platform benchmarks—utilize a "Mean GSD" formulation to standardize operational parameters.

The most rigorous computation of the Mean GSD requires numerically integrating the ground projection for every individual pixel over the underlying digital terrain model, yielding an exact average spatial resolution for that specific exposure.11 However, such pixel-by-pixel integration is computationally prohibitive during the preliminary flight planning phase. Therefore, an approximate mathematical solution based on a systematic grid pattern has become the industry standard.

Empirical research within the EuroSDR project has demonstrated that computing the mean GSD based on a systematic 3x3 pattern of GSDs sampled uniformly across the image plane (the corners, midpoints, and centroid) is highly recommended. This 3x3 sampling matrix yields an overall Mean GSD calculation error of less than 1 centimeter for 99.8% of typical aerial exposures, operating completely independently of the underlying topographic variations of the survey mission.11

Alternatively, some simplified mission planners utilize a scalar Mean GSD that represents the geometric mean of the orthogonal GSD components evaluated strictly at the principal point:

 evaluated at the coordinates 

In high-end multi-sensor systems engineered to geometrically match nadir and oblique resolutions, flight planners utilize the GSD at the principal point as the baseline operational metric. This is done with the explicit mathematical acknowledgement that the near-edge resolution will be strictly higher (capturing finer details) and the far-edge resolution will be strictly lower (capturing coarser details), relying on dense image overlap to cull the lowest-resolution pixels during the generation of the final three-dimensional point cloud.12

## Collinearity Equations and Complex Exterior Orientation Models

The foundational mathematical model of all analytical photogrammetry is the collinearity condition. This condition asserts a strict geometric rule: the perspective center of the camera lens, the two-dimensional image point recorded on the sensor, and the corresponding three-dimensional object point on the ground must all lie on a single, continuous, unbroken straight line in three-dimensional space.13

### The General Fractional Collinearity Equations

For a physical object point defined by ground coordinate system values (X, Y, Z) and an aircraft exposure station located at the spatial coordinates (X₀, Y₀, Z₀), the corresponding two-dimensional image coordinates (x, y) mapped onto a digital sensor with a calibrated focal length f and a principal point offset of (x₀, y₀) are defined by the non-linear fractional collinearity equations:

```
x - x₀ = -f × [r₁₁(X-X₀) + r₂₁(Y-Y₀) + r₃₁(Z-Z₀)] / [r₁₃(X-X₀) + r₂₃(Y-Y₀) + r₃₃(Z-Z₀)]
y - y₀ = -f × [r₁₂(X-X₀) + r₂₂(Y-Y₀) + r₃₂(Z-Z₀)] / [r₁₃(X-X₀) + r₂₃(Y-Y₀) + r₃₃(Z-Z₀)]

where r_ij are elements of the 3×3 rotation matrix R(ω, φ, κ)
```

 13

 13

The parameters denoted  through  represent the nine discrete mathematical elements of the  orthogonal rotation matrix . This matrix precisely defines the angular attitude of the camera sensor relative to the geodetic ground coordinate system at the exact microsecond of shutter actuation.13

### Sequential Derivation of the Rotation Matrix

The rotation matrix  is not an arbitrary array; it is constructed by applying three sequential, elementary mathematical rotations. In standard photogrammetric convention, these consist of a primary rotation  (analogous to pitch) around the flight trajectory's x-axis, a secondary rotation  (analogous to roll) around the y-axis, and a tertiary rotation  (analogous to yaw or heading) around the vertical z-axis.13

The resulting composite matrix is the product of these individual rotation matrices, , and is rigorously defined by the following trigonometric identities 13:

 13  13  13  13  13  13  13  13  13

### Mathematical Integration of Camera Mount Boresight Angles

In a traditional single-camera nadir mission, the rotation matrix  is heavily derived from the aircraft's onboard Inertial Measurement Unit (IMU). However, in a multi-camera oblique system, the optical axes of the four oblique sensors are mechanically fixed at specific declination angles (typically 40° to 45° pitch or roll, depending on the mounting orientation) relative to the central nadir sensor.18 While the IMU records the global, absolute attitude of the aircraft platform (ω, φ, κ), the collinearity equations require the specific, localized attitude of each individual sensor head to correctly project the mathematical rays.

To resolve this discrepancy, advanced flight planning and aerial triangulation software apply a pre-calculated relative rotation matrix R_boresight. This matrix defines the precise, fixed boresight angles and mechanical tilt of each oblique cone relative to the primary IMU reference frame.19 The absolute orientation matrix R_total utilized for a given oblique camera at the moment of exposure is computed as the matrix product of the aircraft's dynamic spatial attitude and the camera's fixed mechanical mount orientation:

```
R_total = R_platform(ω, φ, κ) × R_boresight(Δω, Δφ, Δκ)
```

where R_platform is the IMU-derived aircraft attitude and R_boresight is the fixed mechanical offset of each oblique head.

Flight planners rely heavily on this composite absolute matrix to project the theoretical trapezoidal footprint of the oblique sensors onto a coarse Digital Elevation Model (DEM) during the mission design phase.21 By computationally integrating the target terrain elevation  with the predicted aircraft trajectory  and the dynamically calculated  matrix, the planning software accurately determines exactly where the oblique footprints will intersect the earth. This sophisticated mathematical forecasting ensures that the normal aerodynamic roll and pitch of the aircraft due to turbulence do not inadvertently sweep the oblique footprint off the target area, thereby preventing disastrous data gaps in the oblique coverage over highly variable terrain.22

## Flight Line Spacing and Overlap Computations for 5-Head Systems

Flight planning for a standard 5-head Maltese-cross system—comprising one nadir camera and four oblique cameras oriented at 0°, 90°, 180°, and 270° azimuths relative to the flight path—is a highly constrained, multi-variable mathematical optimization problem. The mission profile must mathematically guarantee gapless coverage and dense multi-view stereoscopic redundancy while simultaneously minimizing unnecessary, redundant flight lines to preserve the economic viability of the aerial survey.23

### Nadir vs. Oblique Overlap Driving Factors

In the historical context of analog photogrammetry, nadir overlap parameters of 60% forward (in-strip) and 30% side (cross-strip) were entirely sufficient to execute a high-quality aerotriangulation block over relatively flat terrain.24 However, the advent of modern dense image matching algorithms and the requirement for artifact-free digital twin generation demand vastly higher levels of mathematical redundancy to resolve complex urban structures, mitigate building lean, and filter out point cloud noise. For nadir-only mapping in urban environments, an 80/80% overlap is heavily recommended to eliminate the relief displacement occlusions inherently caused by tall buildings.25

When transitioning the flight plan to accommodate a 5-head oblique system, the geometric requirements diverge significantly from nadir-only logic. The flight line spacing is fundamentally dictated by the worst-case scenario across all camera heads to ensure gapless stereoscopic coverage of the vertical structures.24

If the flight lines are spaced to achieve an 80% cross-track sidelap strictly for the nadir camera, the side-looking oblique cameras will naturally exhibit extremely high overlap—often mathematically exceeding 90%. This occurs because their broad trapezoidal footprints project significantly further laterally across the flight track than the narrow nadir footprint. However, relying solely on this incidental high overlap is dangerous; the geometric quality of the oblique footprint degrades precipitously at the far edge due to atmospheric occlusion, poor GSD, and extreme incidence angles.

### The Limiting Factor in Flight Spacing Geometry

Therefore, the final overlap requirement is governed by a complex intersection of the nadir camera's positional baseline and the oblique cameras' effective, usable resolution thresholds. The flight planner must mathematically verify that:

- The central nadir camera maintains an absolute minimum of 60% sidelap, though 80% is the strict operational standard for urban environments to ensure the central image contribution area is devoid of blind spots.25

- The high-resolution near-edge of the left-looking oblique footprint from one flight line overlaps adequately with the lower-resolution far-edge of the left-looking oblique footprint from the adjacent parallel flight line, ensuring sufficient tie-point connectivity.

- The cross-track spatial spacing does not mathematically allow gaps in the effective occluded zones caused by extreme building lean, which requires estimating the effective maximum occlusion using the opening angle of the camera against the maximum known building height in the survey area.24

Recent peer-reviewed research on penta-camera configurations published in ISPRS archives indicates that while nadir overlaps are perfectly symmetric across the grid, oblique overlaps are highly asymmetric.26 A continuous and consistent ground coverage requires calculating the overlapping footprint using the Compass Direction (CD) frame analysis, which tracks the coverage of each cardinal view independently.26 In practical implementation, if an urban project requires the generation of high-density 3D meshes, the nadir sensor drives the baseline flight line spacing (typically locked at 80/80%). This inherently conservative spacing consequentially guarantees that the oblique sensors will capture every standing structure from multiple, highly redundant viewing angles, successfully absorbing the varying spatial resolutions inherent to the trapezoidal geometry.25

## Capturing Building Facades and Vertical Surfaces

The primary engineering mandate of an oblique photogrammetry system is the high-fidelity spatial reconstruction and photo-realistic textural mapping of vertical building facades. These vertical elements are inherently occluded, shadowed, or severely geometrically distorted in traditional nadir-only imagery due to the downward-looking perspective.23

### The Minimum Oblique Angle for Effective Texture Capture

The successful mathematical extraction of semantic facade elements—such as windows, architectural protrusions, and balconies—requires an optical axis that strikes the vertical surface at a highly favorable incidence angle. Extensive empirical research, supported by ISPRS benchmarks, demonstrates that typical oblique camera tilt angles must fall between 30° and 45° to provide mathematically usable textures.23 A precise tilt of 40° to 45° from the nadir is generally recognized as the universal mathematical optimum for urban reconstruction.1

This 40-45° optimization represents a carefully calculated geometric compromise:

- A steeper, more vertical angle (e.g., a 20° tilt from nadir) minimizes the trapezoidal footprint distortion and restricts GSD degradation, but fundamentally fails to capture the lower elevations of building facades located within narrow urban canyons due to severe roof occlusion and tight building spacing.29

- A shallower, more horizontal angle (e.g., a 60° tilt from nadir) provides excellent, almost orthogonal views of the vertical facade geometry, but introduces crippling scale variations across the focal plane, drastically reduces the usable footprint due to atmospheric haze interference, and prevents the mapping of deep urban corridors due to masking from adjacent structures.27

### Ensuring Multi-View Coverage of Urban Faces

To generate mathematically reliable dense point clouds via sophisticated algorithms like Semi-Global Matching (SGM), the photogrammetric intersection equations dictate that each target point on a vertical facade must be clearly visible from at least two different exposure stations, exhibiting an adequate baseline-to-height ratio to compute depth accurately.30

A standard 5-head system looking uniformly at the four cardinal directions (0°, 90°, 180°, 270° relative to the airframe) ensures that at least one oblique camera will maintain a highly favorable planimetric angle to any given facade in a standard Manhattan-grid urban layout. Specifically, the spherical mathematics of the four-way coverage guarantee that the planimetric angle—measured between any facade's horizontal normal vector projected to the ground and the camera's line-of-sight vector—is at most 45°.28 This angle is sufficiently acute to reliably extract geometric features without extreme pixel shearing.

To ensure that each individual facade is seen from at least *two* overlapping oblique viewing angles, the flight plan logic implements a boustrophedonic (continuous back-and-forth lawnmower) flight path.33 Because the forward-looking and backward-looking oblique cameras capture the urban scene sequentially along the track, a high forward overlap (e.g., 80%) mathematically guarantees that a facade perpendicular to the flight line will be captured in multiple successive frames by the forward-looking camera as the aircraft approaches the structure, and subsequently captured again by the backward-looking camera as the aircraft passes over and away from the structure.34 For lateral, parallel-facing facades, the tight cross-track spacing dictated by the 80% nadir sidelap guarantees that the side-looking oblique cameras from two adjacent, parallel flight lines maintain high stereoscopic redundancy.18

## Advanced Commercial Sensor Configurations and Scaling Matrices

To mathematically resolve the inherent spatial disparity between nadir and oblique GSDs across the survey area, modern aerial sensor architectures utilize highly tailored, asymmetric focal lengths. A comparative mathematical analysis of the two preeminent commercial systems—the Leica CityMapper-2 and the Phase One PAS 880—illustrates the complex optical engineering required to harmonize multi-view photogrammetric data.

### Focal Length Ratios and Geometric GSD Equalization

If an oblique camera and a nadir camera are mounted in the same pod possessing identical focal lengths and identical pixel pitches, the GSD precisely at the principal point of the oblique image will be substantially larger (lower resolution) than the nadir GSD. This is due entirely to the increased slant range distance, defined as D_slant = H_agl / cos(τ). To mathematically equalize the GSDs at the center of the frames, the oblique focal length f_oblique must be scaled upward relative to the nadir focal length f_nadir by a factor equal to the secant of the tilt angle:

```
f_oblique = f_nadir × sec(τ) = f_nadir / cos(τ)

At τ = 45°: f_oblique = f_nadir × 1.414
```

This proportional focal scaling ensures that the instantaneous field of view (IFOV) of an individual pixel on the oblique sensor subtends the exact same physical ground distance at the 45° principal point as a pixel on the nadir sensor subtends at the true nadir point directly below the aircraft.

### Leica CityMapper-2 Specifications

The Leica CityMapper-2 is a highly advanced hybrid airborne sensor combining a 2 MHz linear-mode Lidar scanner with dual nadir cameras (capturing both RGB and near-infrared spectrums) and four oblique RGB cameras.1 Operating with custom-designed 150-megapixel Backside-Illuminated (BSI) CMOS sensors featuring an incredibly dense 3.76 µm pixel pitch, the optical system utilizes mechanical forward-motion-compensation (FMC) to physically move the sensor during exposure, eliminating motion blur at high operational speeds.1

Leica engineers offer three distinct optical configurations (designated L, S, and H) specifically designed for varying flying heights to maintain a strict, regulatory-compliant GSD (e.g., 2 cm to 10 cm). The focal lengths across these models are strictly engineered to adhere to the geometric scaling principle outlined above:

| **Configuration** | **Nadir Focal Length (RGB ****&**** NIR)** | **Oblique Focal Length (RGB)** | **Target Altitude for 5cm GSD** | **Nadir/Oblique Focal Ratio** |
| --- | --- | --- | --- | --- |
| **CityMapper-2L** (Low) | 71 mm | 112 mm | ~940 m AGL | 1 : 1.57 |
| **CityMapper-2S** (Standard) | 112 mm | 146 mm | ~1490 m AGL | 1 : 1.30 |
| **CityMapper-2H** (High) | 146 mm | 189 mm | ~1940 m AGL | 1 : 1.29 |

Data derived from Leica CityMapper-2 technical specifications.1

In the CityMapper-2L configuration, the resulting mathematical ratio is 1:1.57, which slightly exceeds the theoretical 1.414 baseline requirement. This specific design choice intentionally over-resolves the oblique principal point relative to the nadir view. By doing so, Leica effectively pushes the high-resolution envelope further toward the far, horizon-facing edge of the trapezoidal footprint, mathematically compensating for the accelerated GSD degradation that plagues the outer edges of the image.1 The Field of View (FOV) for the 2L oblique configuration provides a  across-track sweep and a  along-track sweep.1

### Phase One PAS 880 and iXM-MV Specifications

The Phase One PAS 880 represents a formidable alternative, engineered as a large-format nadir and oblique system designed for maximum aerial productivity without the inclusion of Lidar. It integrates a massive 280-megapixel nadir capability alongside four 150-megapixel oblique cameras into a single stabilized pod.37 The system processes data at an exceptionally high capture rate of 2 frames per second, spanning an ultra-wide imaging swath of 20,000 pixels across-track in the nadir view.37

Matching the Leica architecture, Phase One utilizes an identical 3.76 µm pixel size on their proprietary BSI CMOS sensors. The PAS 880 standardizes its complex optics by pairing a 90 mm lens for the central nadir cameras with heavy 150 mm lenses for all four oblique cameras.37

| **Component** | **Sensor Resolution** | **Pixel Size** | **Focal Length** | **Nominal Tilt Angle** |
| --- | --- | --- | --- | --- |
| **Nadir (RGB/NIR)** | 280 MP (Combined) / 150 MP | 3.76 µm | 90 mm | 0° (Vertical) |
| **Oblique (x4 RGB)** | 150 MP (14204 x 10652) | 3.76 µm | 150 mm | 45° |

Data derived from Phase One PAS 880 and iXM series technical specifications.37

The exact focal length ratio for the Phase One PAS 880 is . Once again, the optical engineers have intentionally designed the system to mathematically surpass the basic  slant-range geometric compensation. This ensures that the oblique imagery provides deeply penetrating spatial resolution even at the furthest extremities of the field of view, supporting robust dense image matching on complex facades located deep within the camera's periphery.37

## Bundle Block Adjustment and Tie-Point Matching for Oblique Blocks

The transition from single-camera nadir-only missions to multi-head oblique photogrammetry introduces profound, mathematically dense complexities into the Aerial Triangulation (AT) and Bundle Block Adjustment (BBA) phases. The introduction of large 45° tilt angles severely violates traditional affine image matching assumptions, complicating automated tie-point extraction algorithms and forcing the absolute adoption of rigorous self-calibration routines within the block matrix.

### Tie-Point Matching Challenges and Algorithmic Requirements

In traditional nadir photogrammetry, scale and rotational shear are relatively uniform across the overlapping areas between sequential frames. Traditional feature extraction algorithms, such as the Scale-Invariant Feature Transform (SIFT), easily establish mathematical correspondences by comparing local gradients. However, in complex oblique blocks, a specific ground feature—such as a building corner—captured in the high-resolution near-edge of a forward-looking oblique image will be captured at a vastly different spatial scale, resolution, and affine perspective in the low-resolution far-edge of a backward-looking oblique image from an adjacent flight line.

Extensive empirical experiences from the joint ISPRS/EuroSDR benchmark on multi-platform photogrammetry—specifically analyzing a heavily tested Dortmund dataset captured with an IGI Pentacam—have rigorously highlighted these algorithmic challenges.40 Tie-point matching is generally mathematically successful between overlapping images pointing in the exact same cardinal direction (e.g., matching a forward-oblique image to the subsequent forward-oblique image) and between the nadir and oblique views.40 However, mathematically matching features across different viewing directions (e.g., trying to tie a North-facing oblique image to an East-facing oblique image) remains a highly complex computational problem due to extreme perspective distortion, physical self-occlusions of the buildings, and unresolved spectral ambiguities.40 Advanced algorithms utilizing Affine Scale-Invariant Feature Transforms (ASIFT) are increasingly necessitated to accommodate the severe geometric shear and perspective warping present in the overlapping trapezoidal footprints.41

To mathematically guarantee reliable tie-point extraction under these hostile geometric conditions, overwhelming image overlap is paramount. The EuroSDR benchmark conclusively demonstrated that reducing the flight plan overlap from 80/80% to 60/60% drastically degrades the statistical reliability of the feature matching.40 A high tie-point multiplicity—defined as the total number of individual camera frames in which a single 3D geometric point is successfully observed—is critical. Higher overlap exponentially increases the number of cameras capturing each coordinate 31, allowing the BBA algorithm to aggressively filter outliers and improve measurement precision through highly constrained multi-stereo triangulation.24

### Mathematical Differences in Bundle Block Adjustment (BBA)

The fundamental mathematical objective of the Bundle Block Adjustment is to simultaneously estimate the 3D ground coordinates of millions of tie-points while refining the exterior and interior orientation parameters of all participating cameras. This is achieved by minimizing the re-projection error across the entire block using a highly non-linear least-squares adjustment, typically solved via the Levenberg-Marquardt algorithm.15

For oblique camera blocks, the BBA mathematical formulation differs from nadir-only adjustments in three highly critical ways:

- **Systematic Error Modeling and Interior Self-Calibration:** Oblique cameras exhibit highly pronounced systematic errors due to severe atmospheric refraction gradients acting across the vast depth of field, as well as microscopic mechanical flex in the multi-sensor mountings. Exhaustive research from the EuroSDR benchmark explicitly mandates that the camera's interior orientation parameters—specifically the principal distance, the principal point offset, and the radial/tangential distortion polynomials—must be dynamically estimated during an in-flight self-calibration phase incorporated directly into the BBA matrix. Failing to do so prevents the mathematical exploitation of the system's true geometric potential.40

- **Relative Rotation Matrix Stability Constraints (RRMSC):** While early generations of oblique systems allowed the 5 camera heads to be mathematically adjusted independently during the BBA, modern multi-head optimization algorithms strictly enforce a Relative Rotation Matrix Stability Constraint.45 Because all five camera heads are rigidly bolted into a single physical pod (such as the massive Phase One PAS 880 or the Leica CityMapper-2), the relative spatial offset and angular relationship between the nadir and the four oblique sensors are physically locked. By introducing these rigid mechanical relationships as mathematical constraints within the least-squares normal equations, the BBA algorithm massively reduces the allowable degrees of freedom in the sparse block matrix. This approach drastically accelerates algorithmic convergence and successfully prevents localized block deformation, even over water bodies or dense forests where texture matching is exceptionally poor.45

- **Enhanced Ray Intersection Geometry:** Multi-camera oblique systems inherently provide a significantly stronger, more rigid mathematical ray intersection geometry than nadir-only blocks. The widely dispersed intersecting rays arriving from steep 45° angles provide highly constrained vertical triangulation, minimizing the Z-axis error ellipse. The ISPRS benchmark mathematically confirmed that incorporating all available images (nadir and all four obliques) into a single, rigorous, unified bundle block adjustment significantly improves both planimetric and, most notably, altimetric (height) accuracy across the entire terrain model.26 Statistical analysis of the benchmark revealed that the random errors in the 3D object space for an 80/80% overlap oblique configuration were consistently 2 to 3 times smaller in magnitude than those produced by a 60/60% overlap configuration.40

## Synthesis of Block Adjustment and Mission Efficacy

The deployment of multi-camera oblique photogrammetry systems has irreversibly revolutionized the acquisition of urban spatial data, transitioning the geomatics industry from rudimentary 2.5D topographic mapping to the generation of mathematically rigorous, photo-realistic 3D digital twins. However, this advanced capability demands a highly sophisticated, mathematically sound approach to mission planning and geometric modeling.

The intentional tilt of the oblique sensors generates complex trapezoidal footprints governed by strict fractional collinearity equations. This intersection geometry results in extreme Ground Sample Distance variations that inherently complicate automated feature extraction and scale management. To elegantly resolve this, leading optical sensor manufacturers deploy engineered focal length ratios—such as 150 mm oblique lenses paired with 90 mm nadir lenses—that geometrically balance the spatial resolutions across the 45° incidence angles. Furthermore, flight planning parameters can no longer rely on the traditional 60/30% nadir overlap metrics established during the analog era. Generating mathematically reliable dense image matches on vertical facades mandates a worst-case scenario flight plan, prioritizing an aggressive 80/80% overlap configuration to mathematically guarantee that every structural face is captured from at least two complementary, overlapping oblique viewing angles. As conclusively demonstrated by the ISPRS and EuroSDR benchmarks, strictly adhering to these high-redundancy overlap parameters and utilizing rigidly constrained, self-calibrating Bundle Block Adjustments are absolute prerequisites for achieving the sub-centimeter spatial accuracies demanded by modern smart city infrastructure and advanced civil engineering applications.

#### Works cited

- Leica CityMapper-2 More information, smarter decisions, accessed March 24, 2026, [https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica%20citymapper-2%20ds.ashx](https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica%20citymapper-2%20ds.ashx)

- (PDF) Building Footprints Extraction from Oblique Imagery - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/263446651_Building_Footprints_Extraction_from_Oblique_Imagery](https://www.researchgate.net/publication/263446651_Building_Footprints_Extraction_from_Oblique_Imagery)

- comparison between two generic 3d building reconstruction approaches – point cloud based - Semantic Scholar, accessed March 24, 2026, [https://pdfs.semanticscholar.org/87d4/1abc807e5b9a74bf4d2a311cb78aeecf5935.pdf](https://pdfs.semanticscholar.org/87d4/1abc807e5b9a74bf4d2a311cb78aeecf5935.pdf)

- How to calculate Ground Sampling Distance (GSD) - Inertial Labs, accessed March 24, 2026, [https://inertiallabs.com/how-to-calculate-ground-sampling-distance/](https://inertiallabs.com/how-to-calculate-ground-sampling-distance/)

- (PDF) Exterior orientation estimation of oblique aerial imagery using vanishing points, accessed March 24, 2026, [https://www.researchgate.net/publication/303887730_Exterior_orientation_estimation_of_oblique_aerial_imagery_using_vanishing_points](https://www.researchgate.net/publication/303887730_Exterior_orientation_estimation_of_oblique_aerial_imagery_using_vanishing_points)

- Oblique Aerial Images: Geometric Principles, Relationships and Definitions - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2673-8392/4/1/19](https://www.mdpi.com/2673-8392/4/1/19)

- Styliani VERYKOKOU and Charalabos IOANNIDIS, Greece - International Federation of Surveyors, accessed March 24, 2026, [https://www.fig.net/resources/proceedings/fig_proceedings/fig2015/papers/ts06e/TS06E_verykokou_ioannidis_7516.pdf](https://www.fig.net/resources/proceedings/fig_proceedings/fig2015/papers/ts06e/TS06E_verykokou_ioannidis_7516.pdf)

- Variations in pixel size in oblique images. O…perspective centre,... - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/figure/ariations-in-pixel-size-in-oblique-images-Operspective-centre-Nnadir-point_fig5_259758858](https://www.researchgate.net/figure/ariations-in-pixel-size-in-oblique-images-Operspective-centre-Nnadir-point_fig5_259758858)

- GSD Calculator – Mastering Spatial Resolution for Earth Observation Satellites​ - simera-sense.com, accessed March 24, 2026, [https://simera-sense.com/news/gsd-calculator-mastering-spatial-resolution-for-earth-observation-satellites/](https://simera-sense.com/news/gsd-calculator-mastering-spatial-resolution-for-earth-observation-satellites/)

- (PDF) Scales of oblique photographs updated - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/377051918_Scales_of_oblique_photographs_updated](https://www.researchgate.net/publication/377051918_Scales_of_oblique_photographs_updated)

- (PDF) Medium format digital cameras—a EuroSDR project - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/228763845_Medium_format_digital_cameras-a_EuroSDR_project](https://www.researchgate.net/publication/228763845_Medium_format_digital_cameras-a_EuroSDR_project)

- Ground Sample Distance (GSD): Definition, Importance & Calculation - JOUAV, accessed March 24, 2026, [https://www.jouav.com/blog/ground-sample-distance.html](https://www.jouav.com/blog/ground-sample-distance.html)

- Photogrammetry Lect. #6 Collinearity equation and Exterior orientation, accessed March 24, 2026, [https://academics.su.edu.krd/public/profiles/haval.sadeq/teaching/teaching-578-51379-1716638224-2.pdf](https://academics.su.edu.krd/public/profiles/haval.sadeq/teaching/teaching-578-51379-1716638224-2.pdf)

- GE 119 – PHOTOGRAMMETRY 2 - WordPress.com, accessed March 24, 2026, [https://bsgecarsu.files.wordpress.com/2017/10/ge-119_photogrammetry-2_topic-4_v10252017g_for_web.pdf](https://bsgecarsu.files.wordpress.com/2017/10/ge-119_photogrammetry-2_topic-4_v10252017g_for_web.pdf)

- Collinearity equation - Wikipedia, accessed March 24, 2026, [https://en.wikipedia.org/wiki/Collinearity_equation](https://en.wikipedia.org/wiki/Collinearity_equation)

- Photogrammetry II - Rotation Matrix, accessed March 24, 2026, [https://coeng.uobaghdad.edu.iq/wp-content/uploads/sites/3/2021/02/Photogrammetry-II-lecture-7.pdf](https://coeng.uobaghdad.edu.iq/wp-content/uploads/sites/3/2021/02/Photogrammetry-II-lecture-7.pdf)

- Collinearity Condition in Photogrammetry | PDF | Equations | Line (Geometry) - Scribd, accessed March 24, 2026, [https://www.scribd.com/presentation/500827278/Kuliah-7-Fotogrammetri-I-Collinearity-Condition](https://www.scribd.com/presentation/500827278/Kuliah-7-Fotogrammetri-I-Collinearity-Condition)

- The use of oblique and vertical images for 3D urban modelling - - Nottingham ePrints, accessed March 24, 2026, [https://eprints.nottingham.ac.uk/11551/1/Ahmed_thesis.pdf](https://eprints.nottingham.ac.uk/11551/1/Ahmed_thesis.pdf)

- (PDF) Generating Virtual Images from Oblique Frames - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/258811397_Generating_Virtual_Images_from_Oblique_Frames](https://www.researchgate.net/publication/258811397_Generating_Virtual_Images_from_Oblique_Frames)

- Integrated Sensor Orientation at ICC, mathematical models and experience, accessed March 24, 2026, [https://www.ipi.uni-hannover.de/fileadmin/ipi/publications/oeepe_cd.pdf](https://www.ipi.uni-hannover.de/fileadmin/ipi/publications/oeepe_cd.pdf)

- _Irontrack-Manifest

- Ground sampling distance (GSD) in photogrammetry - Pix4D Documentation, accessed March 24, 2026, [https://support.pix4d.com/hc/en-us/articles/202559809](https://support.pix4d.com/hc/en-us/articles/202559809)

- Oblique Imaging in Aerial Photogrammetry: Principles, Advantages, and Practical Implementation - gnss.ae, accessed March 24, 2026, [https://gnss.ae/oblique-imaging-in-aerial-photogrammetry-principles-advantages-and-practical-implementation/](https://gnss.ae/oblique-imaging-in-aerial-photogrammetry-principles-advantages-and-practical-implementation/)

- Nadir Image Overlap - SURE Knowledge Base, accessed March 24, 2026, [https://docs.nframes.com/knowledge-base/image-acquisition-and-overlap/nadir-image-overlap](https://docs.nframes.com/knowledge-base/image-acquisition-and-overlap/nadir-image-overlap)

- Choosing the right nadir image overlap for optimal DSM and True Ortho generation - Esri Community, accessed March 24, 2026, [https://community.esri.com/t5/arcgis-reality-blog/choosing-the-right-nadir-image-overlap-for-optimal/ba-p/1608500](https://community.esri.com/t5/arcgis-reality-blog/choosing-the-right-nadir-image-overlap-for-optimal/ba-p/1608500)

- Optimizing Bundle Block Adjustment for High-Overlap Small-Format Multi-Head Camera Systems, accessed March 24, 2026, [https://isprs-archives.copernicus.org/articles/XLVIII-2-W7-2024/17/2024/isprs-archives-XLVIII-2-W7-2024-17-2024.pdf](https://isprs-archives.copernicus.org/articles/XLVIII-2-W7-2024/17/2024/isprs-archives-XLVIII-2-W7-2024-17-2024.pdf)

- 47. Analysis of camera orientation variation in airborne photogrammetry: images under tilt (roll-pitch-yaw) angles - Extrica, accessed March 24, 2026, [https://www.extrica.com/article/15116/pdf](https://www.extrica.com/article/15116/pdf)

- Lukas Schack - Deutsche Geodätische Kommission, accessed March 24, 2026, [https://dgk.badw.de/fileadmin/user_upload/Files/DGK/docs/c-784.pdf](https://dgk.badw.de/fileadmin/user_upload/Files/DGK/docs/c-784.pdf)

- Building extraction from oblique airborne imagery based on robust façade detection - https ://ris.utwen te.nl, accessed March 24, 2026, [https://ris.utwente.nl/ws/files/287611790/1_s2.0_S0924271611001602_main.pdf](https://ris.utwente.nl/ws/files/287611790/1_s2.0_S0924271611001602_main.pdf)

- Agisoft Metashape User Manual - Professional Edition, Version 1.5, accessed March 24, 2026, [https://www.agisoft.com/pdf/metashape-pro_1_5_en.pdf](https://www.agisoft.com/pdf/metashape-pro_1_5_en.pdf)

- Practical Guidelines for Performing UAV Mapping Flights with Snapshot Sensors - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2072-4292/17/4/606](https://www.mdpi.com/2072-4292/17/4/606)

- (PDF) Real-Time Visibility-Based Fusion of Depth Maps - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/221111474_Real-Time_Visibility-Based_Fusion_of_Depth_Maps](https://www.researchgate.net/publication/221111474_Real-Time_Visibility-Based_Fusion_of_Depth_Maps)

- Overlap & Flight Pattern | Drone Data Processing - Aerotas, accessed March 24, 2026, [https://www.aerotas.com/overlap-flight-pattern](https://www.aerotas.com/overlap-flight-pattern)

- Unmanned Aerial Vehicles (UAVs) for Physical Progress Monitoring of Construction - MDPI, accessed March 24, 2026, [https://www.mdpi.com/1424-8220/21/12/4227](https://www.mdpi.com/1424-8220/21/12/4227)

- New Leica CityMapper Configuration for Challenging Digital Twin Creation | GIM International, accessed March 24, 2026, [https://www.gim-international.com/content/news/new-leica-citymapper-configuration-for-challenging-digital-twin-creation?output=pdf](https://www.gim-international.com/content/news/new-leica-citymapper-configuration-for-challenging-digital-twin-creation?output=pdf)

- Leica CityMapper-2 High-Performance Urban Mapping Sensor, accessed March 24, 2026, [https://leica-geosystems.com/products/airborne-systems/hybrid-sensors/leica-citymapper-2](https://leica-geosystems.com/products/airborne-systems/hybrid-sensors/leica-citymapper-2)

- Phase One PAS 880 - Large-Format Nadir & Oblique Aerial System, accessed March 24, 2026, [https://www.phaseone.com/2020/12/15/pr-pas880-oblique-camera-system/](https://www.phaseone.com/2020/12/15/pr-pas880-oblique-camera-system/)

- Phase One Announces Next-Generation Aerial Solutions Enhanced with Near Infrared Capabilities, accessed March 24, 2026, [https://www.phaseone.com/2022/05/18/phase-one-announces-next-generation-aerial-solutions-enhanced-with-near-infrared-capabilities/](https://www.phaseone.com/2022/05/18/phase-one-announces-next-generation-aerial-solutions-enhanced-with-near-infrared-capabilities/)

- iXM-MV150F | iXM-MV100 - PhaseOne, accessed March 24, 2026, [https://www.phaseone.com/wp-content/uploads/2024/01/ixm-mv150f_brochure.pdf](https://www.phaseone.com/wp-content/uploads/2024/01/ixm-mv150f_brochure.pdf)

- (PDF) ORIENTATION OF OBLIQUE AIRBORNE IMAGE SETS – EXPERIENCES FROM THE ISPRS/EUROSDR BENCHMARK ON MULTI-PLATFORM PHOTOGRAMMETRY - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/307530582_ORIENTATION_OF_OBLIQUE_AIRBORNE_IMAGE_SETS_-_EXPERIENCES_FROM_THE_ISPRSEUROSDR_BENCHMARK_ON_MULTI-PLATFORM_PHOTOGRAMMETRY](https://www.researchgate.net/publication/307530582_ORIENTATION_OF_OBLIQUE_AIRBORNE_IMAGE_SETS_-_EXPERIENCES_FROM_THE_ISPRSEUROSDR_BENCHMARK_ON_MULTI-PLATFORM_PHOTOGRAMMETRY)

- Dense Point Cloud Extraction From Oblique Imagery - Rochester Institute of Technology, accessed March 24, 2026, [https://home.cis.rit.edu/~cnspci/references/theses/masters/zhang2013.pdf](https://home.cis.rit.edu/~cnspci/references/theses/masters/zhang2013.pdf)

- Photogrammetry: Triangulation and bundle block adjustment (part 3) - Geodelta, accessed March 24, 2026, [https://www.geodelta.com/en/articles/photogrammetry-triangulation-and-bundle-block-adjustment](https://www.geodelta.com/en/articles/photogrammetry-triangulation-and-bundle-block-adjustment)

- Exterior orientation estimation of oblique aerial images using SfM-based robust bundle adjustment | Request PDF - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/342573795_Exterior_orientation_estimation_of_oblique_aerial_images_using_SfM-based_robust_bundle_adjustment](https://www.researchgate.net/publication/342573795_Exterior_orientation_estimation_of_oblique_aerial_images_using_SfM-based_robust_bundle_adjustment)

- CAMERA CALIBRATION MODELS AND METHODS FOR CORRIDOR MAPPING WITH UAVS, accessed March 24, 2026, [https://isprs-annals.copernicus.org/articles/V-1-2020/231/2020/isprs-annals-V-1-2020-231-2020.pdf](https://isprs-annals.copernicus.org/articles/V-1-2020/231/2020/isprs-annals-V-1-2020-231-2020.pdf)

- Generating Virtual Images from Oblique Frames - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2072-4292/5/4/1875](https://www.mdpi.com/2072-4292/5/4/1875)

- Using Relative Orientation Constraints to Produce Virtual Images from Oblique Frames - SciSpace, accessed March 24, 2026, [https://scispace.com/pdf/using-relative-orientation-constraints-to-produce-virtual-4eluyovgdk.pdf](https://scispace.com/pdf/using-relative-orientation-constraints-to-produce-virtual-4eluyovgdk.pdf)