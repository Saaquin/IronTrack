# Comprehensive Technical Analysis of the USGS 3D Elevation Program (3DEP) Data Access Architecture

## 1. Introduction and The Paradigm Shift in National Elevation Data

The United States Geological Survey (USGS) 3D Elevation Program (3DEP) represents the most ambitious and comprehensive topographic data infrastructure initiative in the history of the United States. Designed to replace the legacy National Elevation Dataset (NED) framework that operated from 2000 through 2015, the 3DEP initiative was conceived to systematically collect three-dimensional elevation data across the conterminous United States (CONUS), Alaska, Hawaii, and all U.S. territories.1 By leveraging high-density light detection and ranging (lidar) and Interferometric Synthetic Aperture Radar (IfSAR), the program establishes a foundational geospatial baseline that supports complex hydrological modeling, autonomous navigation, precision agriculture, and infrastructure planning.1

For modern automated software systems—particularly headless computational engines tasked with high-throughput geospatial processing, machine learning feature extraction, or autonomous flight planning—programmatic access to 3DEP is a gating requirement for accurate terrain awareness.5 Historically, interacting with national elevation data required human-in-the-loop navigation of fragmented File Transfer Protocol (FTP) servers and the mass downloading of proprietary, unoptimized raster formats such as ArcGrid, GridFloat, or ERDAS IMG.6 This legacy paradigm posed severe bottlenecks for distributed computing and automated continuous integration pipelines, which require deterministic, high-speed access to specific spatial extents without the overhead of processing multi-gigabyte files locally.6

Recognizing this critical bottleneck and the broader industry paradigm shift toward cloud-native geospatial processing, the USGS initiated a massive architectural modernization starting in late 2019.6 This modernization transitioned the primary distribution format of all 3DEP Digital Elevation Models (DEMs) to Cloud Optimized GeoTIFFs (COGs) hosted on Amazon Web Services (AWS).6 Simultaneously, the underlying lidar point clouds were migrated to LASzip (LAZ) and Cloud Optimized Point Cloud (COPC) formats within the AWS Open Data Registry.8 Furthermore, the introduction of the SpatioTemporal Asset Catalog (STAC) specification has revolutionized data discovery, enabling software systems to query massive planetary-scale repositories using standardized JSON APIs.10

This report delivers an exhaustive technical analysis of the 3DEP data access architecture tailored specifically for programmatic ingestion. The analysis delineates the exact data access pathways across various application programming interfaces (APIs), specifies the file formats and spatial tiling conventions across the entire resolution hierarchy, details the complex geodetic considerations surrounding vertical datums and geoid models, and outlines a robust software engineering workflow for integrating these datasets via the Rust programming language. The insights provided herein aim to equip systems architects with the precise logic required to automate terrain modeling seamlessly across varied geographic regions and resolution limits.

## 2. Comprehensive Data Access Pathways and API Architectures

Automated software systems have multiple architectural pathways to discover, query, and ingest 3DEP DEM data. Each pathway serves distinct operational use cases, ranging from legacy metadata discovery to high-speed, parallelized raster streaming within cloud environments. Selecting the appropriate API depends entirely on the system's execution context, payload requirements, and tolerance for external service dependencies.

### 2.1 The National Map (TNM) Access API (v2)

The National Map (TNM) Access API is the official RESTful web service engineered by the USGS to expose all pre-staged downloadable products.12 While this API serves as the backend infrastructure for the visual TNM Download Client utilized by human operators, it is explicitly designed for programmatic HTTP GET and POST requests by automated clients.13

The fundamental architecture of the TNM Access API revolves around a series of interconnected endpoints that categorize geospatial data by dataset themes, individual products, and spatial extents. The base URL for the version 2 API architecture is routed through https://tnmaccess.nationalmap.gov/api/v1/.12

To locate 3DEP data programmatically, a software system must issue an HTTP GET request to the /products endpoint.12 This endpoint requires a spatial bounding box (bbox) and dataset identifiers. For instance, to query the 1/3 arc-second data layer, the query parameters must rigorously define the spatial envelope as bbox=minX,minY,maxX,maxY and specify the exact dataset tag corresponding to the desired resolution.14 The API responds with a structured JSON payload containing the direct download URLs—which predominantly route back to the USGS S3 buckets—and associated metadata including publication dates and product footprints.15 A parallel endpoint, /datasets, organizes products by thematic collections, which is vital for systems needing to dynamically resolve the specific programmatic keys required to filter 1-meter versus 1/3 arc-second data prior to executing the /products search.12 Furthermore, the API provides a /map_services endpoint that delivers Open Geospatial Consortium (OGC) compliant Web Map Service (WMS) and Web Coverage Service (WCS) links for dynamic visualization, although these are generally avoided for raw data ingestion due to computational overhead.12

| **System Component** | **Base URL** | **Primary Functionality** | **Operational Use Case** |
| --- | --- | --- | --- |
| **TNM Base API** | https://tnmaccess.nationalmap.gov/api/v1/ | Root RESTful service | Programmatic initialization |
| **Products Endpoint** | /products | Query individual downloadable files | Bounding box spatial searches |
| **Datasets Endpoint** | /datasets | Thematic collection indexing | Resolving resolution identifiers |
| **Services Endpoint** | /map_services | WMS/WCS OGC standard links | Dynamic visual rendering |

### 2.2 AWS Open Data Registry and Direct S3 Bucket Ingestion

To facilitate cloud-native computing and reduce the latency associated with traditional HTTP downloads, the USGS mirrors the entirety of the 3DEP data holdings on Amazon Web Services. This represents the most efficient, lowest-latency pathway for automated systems operating within the cloud or those leveraging HTTP GET Range requests to stream sub-sections of rasters.6

The finished, bare-earth DEM products—representing the final interpolated terrain grids—are hosted in the prd-tnm S3 bucket.17 Systems can access this repository via the URL pattern https://prd-tnm.s3.amazonaws.com/.17 The directory structure utilizes a strict prefix system, generally following the pattern StagedProducts/Elevation/ followed by the resolution identifier, such as 1m/Projects/ for high-resolution project data or 13/ for the 1/3 arc-second continuous layer.17 This is a public bucket, allowing direct, unauthenticated HTTP access to the Cloud Optimized GeoTIFFs.

Conversely, the raw lidar point clouds are partitioned into two distinct buckets under the AWS Open Data Sponsorship Program, managed in collaboration with Hobu, Inc..20 The first is the Public Entwine Point Tiles (EPT) bucket, accessible via the Amazon Resource Name (ARN) arn:aws:s3:::usgs-lidar-public.9 This bucket contains data converted into EPT, a lossless, streamable octree format ideal for WebGL visualization and bounding-box extractions without requiring full file downloads.9 Accessing this bucket requires no AWS account, allowing systems to use the --no-sign-request flag during programmatic calls.9 The second repository is the Requester Pays bucket for the raw LAZ 1.4 project files, located at arn:aws:s3:::usgs-lidar.9 Because the sheer volume of raw point cloud data incurs massive egress costs, this bucket enforces a Requester Pays protocol. Automated systems targeting this repository must actively provide AWS credentials and assume the financial responsibility for bandwidth utilization, explicitly passing the --request-payer requester flag in SDK calls.21

### 2.3 STAC-Compliant Endpoints and Ecosystems

The SpatioTemporal Asset Catalog (STAC) specification has rapidly emerged as the definitive industry standard for indexing and querying cloud-native geospatial data. For automated systems, querying a STAC API is vastly superior to parsing custom JSON structures from the TNM API or navigating nested S3 prefixes, as STAC provides a universally standardized schema for spatial, temporal, and band-level metadata.10

The most robust STAC access point for 3DEP data is provided by the Microsoft Planetary Computer (MPC), which hosts a comprehensive, free STAC API that indexes 3DEP data mirrored on the Azure cloud.22 The MPC STAC API is accessible at the base URL https://planetarycomputer.microsoft.com/api/stac/v1.22 The catalog organizes the data into distinct collections tailored for different ingestion needs. The 3dep-seamless collection indexes the 10-meter (1/3 arc-second) and 30-meter (1 arc-second) seamless DEM layers across the continent.23 For higher resolution modeling, the 3dep-lidar-dtm-native collection indexes Cloud Optimized GeoTIFFs of the Digital Terrain Model generated directly from the lidar point clouds using vendor-provided ground classifications.8 Additionally, the 3dep-lidar-copc collection provides indexed access to the raw point clouds formatted as Cloud Optimized Point Clouds.25

Concurrently, the USGS maintains an experimental and official STAC catalog specific to the lidar point clouds hosted on AWS. This static STAC catalog is accessible at https://usgs-lidar-stac.s3-us-west-2.amazonaws.com/ept/catalog.json.16 While static catalogs lack the dynamic query capabilities of a full STAC API server, they provide an immutable, easily traversable JSON tree that can be recursively parsed by highly parallelized ingestion engines looking to map the entire AWS usgs-lidar-public bucket.16

### 2.4 The USGS ScienceBase Catalog and Third-Party Aggregators

ScienceBase functions as the comprehensive data management system for the USGS, built on a REST service architecture that utilizes a standardized JSON item model for all scientific outputs.26 Items in ScienceBase are accessed by appending ?format=json to the standard item URL, such as https://www.sciencebase.gov/catalog/item/{item_id}?format=json.26 While less frequently used for the high-speed streaming of heavy raster payloads compared to S3 or STAC, ScienceBase remains an indispensable resource for accessing deep programmatic metadata, project-level provenance, historical dataset lineage, and exhaustive cross-references to original contract specifications.26

Additionally, academic and commercial aggregators like OpenTopography provide specialized APIs that interact with USGS systems on behalf of the user. OpenTopography offers a robust API for accessing 1-meter, 1/3 arc-second, and 1 arc-second 3DEP DEMs.27 Their system leverages the USGS National Map API and AWS cloud storage to subset rasters on-the-fly, returning a seamless raster mosaic strictly clipped to the user's requested bounding box.27 While highly convenient, this pathway requires a registered API key and imposes strict volumetric limits—for instance, a maximum request size of 250 square kilometers for 1-meter data and a daily limit of 50 to 200 calls depending on academic status.28 Consequently, while useful for lightweight applications, enterprise-scale automated systems are better served by directly interacting with the AWS S3 buckets or the Planetary Computer STAC endpoints to avoid these rate limits.

## 3. Topographic Resolutions, File Formats, and Cloud Interoperability

The 3DEP product suite is intentionally stratified, offering multiple horizontal resolutions designed to satisfy diverse geodetic, hydrological, and operational planning applications.2 Understanding the complex interplay between spatial resolution, coverage characteristics (project-based versus seamless mapping), and underlying file formats is absolutely critical for constructing resilient automated ingestion logic.

### 3.1 The Digital Elevation Model Product Hierarchy

The USGS defines elevation products primarily through angular measurements (arc-seconds) mapped to geographic coordinate systems, though highest-resolution products utilize metric measurements on projected grids.

| **Resolution Category** | **Approximate Ground Spacing** | **Coverage Characteristics** | **Primary Distribution Format** | **Geographic Availability** |
| --- | --- | --- | --- | --- |
| **1-Meter S1M** | 1.0 meters | Seamless Continuous Matrix | Cloud Optimized GeoTIFF | Expanding across CONUS, HI, PR 30 |
| **1-Meter Project** | 1.0 meters | Project-Based, Overlapping | COG / ERDAS IMG | CONUS (Dependent on QL2+ lidar) 7 |
| **1/9 arc-second** | ~3.0 meters | Project-Based | ERDAS IMG | ~25% of CONUS (Legacy acquisitions) 2 |
| **5-Meter** | 5.0 meters | Seamless Continuous Matrix | GeoTIFF / ArcGrid | Alaska explicitly (IfSAR derived) 1 |
| **1/3 arc-second** | ~10.0 meters | Seamless Continuous Matrix | Cloud Optimized GeoTIFF | CONUS, HI, Territories, coastal AK 4 |
| **1 arc-second** | ~30.0 meters | Seamless Continuous Matrix | Cloud Optimized GeoTIFF | CONUS, AK, HI, Canada, Mexico 2 |
| **2 arc-second** | ~60.0 meters | Seamless Continuous Matrix | GeoTIFF | Alaska exclusively (IfSAR derived) 2 |

The 1/3 arc-second and 1 arc-second layers are the historic workhorses of the program, providing seamless, medium-resolution grids that are continuously updated as superior source data becomes available.2 The 1/9 arc-second dataset represents a legacy effort to provide higher resolution data prior to the widespread adoption of 1-meter mapping; it is effectively stagnant, covering only about 25 percent of the nation and predominantly distributed in the older ERDAS IMG format.2

### 3.2 The Architectural Shift to Cloud Optimized GeoTIFFs (COGs)

Between late 2019 and early 2020, the USGS executed a fundamental deprecation of legacy distribution formats—such as Esri ArcGrid, GridFloat, and standard uncompressed GeoTIFFs—for all newly published primary DEM products, migrating the national archive entirely to the Cloud Optimized GeoTIFF (COG) format.6

A COG is fundamentally a standard, fully compliant GeoTIFF file, but its internal byte architecture is rigorously organized. A COG mandates that image overviews (reduced-resolution pyramids) and spatial metadata are written at the very beginning of the file header, followed by the pixel data which is spatially tiled rather than stored in monolithic scanning strips.6

For a headless compute engine or an autonomous planning application, this internal architecture is revolutionary.5 By issuing standard HTTP GET Range requests to the AWS prd-tnm bucket or the Planetary Computer endpoints, the software can download the file header, calculate the exact byte offsets corresponding to a specific geographic bounding box, and retrieve only those compressed blocks over the network.6 This allows the engine to extract high-resolution 1-meter elevation data for a hyper-localized 500-meter drone flight corridor without ever downloading the surrounding multi-gigabyte 10x10 km tile. This mechanism radically reduces physical memory consumption, virtually eliminates input/output (I/O) bottlenecks, and drastically mitigates network latency.6 Currently, the 1-meter Seamless (S1M), 1/3 arc-second, and 1 arc-second products natively and exclusively utilize the COG format on AWS.2

### 3.3 The Distinctions Between Seamless and Project-Based 1-Meter Data

A critical architectural distinction that automated systems must accommodate is the fundamental difference between legacy project-based 1-meter DEMs and the newly introduced Seamless 1-Meter (S1M) DEM product.30

Historically, 1-meter DEMs were published precisely as they were collected and delivered by regional lidar contractors.2 This project-based distribution model resulted in thousands of overlapping tiles, differing collection dates that caused severe elevation mismatches across contiguous water surfaces, and the use of varying localized coordinate systems depending on the specific municipal or state contract.30 For automated systems, combining two adjacent project-based tiles often required computationally expensive edge-matching, artifact removal, and on-the-fly coordinate reprojection.

To resolve these systemic operational frictions, the USGS developed the S1M dataset, funded partially by the Inflation Reduction Act, with aggressive production accelerating through 2025 and 2026.30 The S1M utilizes advanced geostatistical blending algorithms—specifically the methodologies defined by Cushing and Tyler (2024)—to merge trillions of disparate lidar points into a singular surface.30 This blending eliminates the gaps and slivers caused by inconsistent contract boundaries and avoids the step-artifacts introduced by differing vertical datums.30 Furthermore, large data voids in the S1M (wider than 10 meters) are automatically backfilled by the USGS using 1/9 or 1/3 arc-second data, ensuring an absolutely continuous surface matrix.31

For automated systems, prioritizing the S1M product over legacy project-based 1-meter data ensures the ingestion of true Analysis Ready Data (ARD). The S1M is delivered alongside robust spatial and textual metadata embedded within an open-source GeoPackage format, detailing the exact source lineage and blending steps utilized for every pixel cluster, thereby ensuring complete scientific reproducibility.30

## 4. Spatial Tiling Schemes and Standardized Nomenclature

Automated discovery and sequential processing of 3DEP data require deep programmatic integration with the USGS spatial tiling and nomenclature schemas. These schemas dictate how continuous spatial data is physically partitioned, and they vary significantly depending on the resolution and seamlessness of the target dataset.

### 4.1 1/3 and 1 arc-second Geographic Seamless Tiles

The medium-resolution seamless datasets, specifically the 1/3 and 1 arc-second products, are organized into standardized 1° × 1° geographic blocks based on latitude and longitude.33

The naming convention for these files is strictly positional and highly predictable. Following the format modernization, a file representing the 1/3 arc-second data for the block with a nominal southwest corner at 44° North and 71° West is stringently named USGS_13_n44w071.tif.36 In this schema, USGS denotes the distributing agency, 13 specifies the 1/3 arc-second resolution (similarly, 1 denotes the 1 arc-second product), and n44w071 defines the 1-degree geographic bounding box.36

It is critical for geospatial developers to note that while the filename indicates a nominal Northwest or Southwest corner, the USGS intentionally includes a slight buffer—known as an overedge—in the physical raster file.37 Therefore, the nominal corner listed in the filename is not the exact mathematical boundary of the pixel matrix. Software ingestion engines must strictly read the internal GeoTIFF spatial metadata tags (GeoKeys) to determine exact sub-pixel alignment and coordinate boundaries rather than blindly relying on the filename parser.

| **Tiling Scheme Characteristic** | **Medium Resolution (1/3 ****&**** 1 arc-second)** | **Seamless 1-Meter (S1M)** | **Project-Based 1-Meter** |
| --- | --- | --- | --- |
| **Grid Extent** | 1° × 1° blocks | 10 km × 10 km grids | 10,000 m × 10,000 m |
| **Coordinate Base** | Geographic (Latitude/Longitude) | Projected (Meters) | Projected (Meters) |
| **Primary CRS** | NAD83 (EPSG:4269) | USA Contiguous Albers (EPSG:6350) | Local UTM Zones |
| **Overedge Padding** | Included | Seamlessly Blended | Overlapping Boundaries |

### 4.2 The 1-Meter Seamless (S1M) Projected Tiling Scheme

Because high-resolution 1-meter data requires exponentially more digital storage per square kilometer, utilizing 1° × 1° blocks is computationally infeasible and would result in files too massive for efficient network transfer. Consequently, the newly minted S1M product utilizes a uniform 10 kilometer by 10 kilometer tiling scheme.30

Crucially, rather than relying on geographic angular coordinates (Latitude/Longitude), the S1M tiling matrix is anchored entirely in projected Cartesian coordinates. All S1M tiles utilize a single, universally unified projection: the North American Datum of 1983 (2011) Contiguous USA Albers Equal Area Conic, cataloged under EPSG:6350.31

This represents a massive architectural advantage for software engineering. By standardizing horizontal arrays and vertical elevation values entirely in meters, spatial area calculations—such as generating photogrammetric flight line spacing, calculating swath width overlaps, or determining volumetric displacement—are geometrically accurate straight out of the file.5 The engine is not forced to perform complex, computationally expensive on-the-fly geographic-to-projected Coordinate Reference System (CRS) transformations.5 The naming convention for S1M predictably incorporates the EPSG:6350 coordinate locations of the tile boundaries, allowing systems to algorithmically predict required filenames based on spatial bounding boxes without executing prior API discovery queries.30

### 4.3 Legacy 1-Meter Project-Based Tiles

In contrast, the legacy project-based 1-meter tiles are organized into localized 10,000 × 10,000 meter arbitrary extents.33 Unlike the S1M, they are strictly distributed in the specific Universal Transverse Mercator (UTM) zone in which they were originally collected by the contractor, aligned to the NAD83 datum.32

This introduces a significant programmatic hurdle: if an aerial survey area or a computational bounding box crosses two distinct UTM zones, the legacy data is delivered independently in both zones.32 This creates localized overlaps where interpolated elevation values might slightly differ due to grid rotation and resampling artifacts.32 Software ingesting these legacy tiles must be inherently capable of dynamically detecting UTM zone boundaries, executing localized reprojections into a unified spatial working area, and algorithmically resolving pixel-value conflicts within the overlapping domains.

## 5. Geodetic Frameworks: Vertical Datums, Geoid Models, and Global Blending

For automated systems designed to fuse high-resolution domestic 3DEP data with global elevation datasets—such as the Copernicus GLO-30 DEM or the Shuttle Radar Topography Mission (SRTM) outputs—rigorous mathematical handling of vertical datums is non-negotiable.5 A naive programmatic mosaic of 3DEP and Copernicus data matrices will invariably result in physical vertical discrepancies ranging from tens of centimeters to several meters. In the context of autonomous navigation, flood risk modeling, or low-altitude flight planning, such uncorrected vertical steps represent catastrophic systemic failures.

### 5.1 Understanding the 3DEP Orthometric Vertical Reference

All bare-earth elevation values within the CONUS 3DEP datasets are measured strictly in meters and are physically referenced to the North American Vertical Datum of 1988 (NAVD88).32 NAVD88 is an *orthometric* height system—often conceptually simplified as height above mean sea level—that is physically tied to a vast network of gravity-adjusted, leveled physical benchmarks spread across the North American continent.39

However, modern topographic mapping utilizes lidar sensors mounted on aerial platforms equipped with Global Navigation Satellite Systems (GNSS). These GNSS systems do not natively measure orthometric height; they measure *ellipsoidal* height relative to a mathematically perfectly smooth reference ellipsoid, specifically NAD83 or WGS84.38 To mathematically convert the raw ellipsoidal heights (h) collected by the sensor into the published orthometric heights (H) expected by civil engineers, the USGS must apply a complex hybrid geoid model (N) generated by the National Geodetic Survey (NGS).41

The foundational mathematical relationship is defined as:

```
h = H + N

where:
  h = ellipsoidal height (from GNSS)
  H = orthometric height (NAVD88, published in 3DEP)
  N = geoid undulation (from GEOID18 hybrid model)
```

Which rearranges to the functional transformation:

```
H = h - N
```

### 5.2 The Transition from GEOID12B to GEOID18

Historically, 3DEP data processing utilized the GEOID12B model to bridge this geodetic gap.42 However, modern standardizations—specifically the production of the S1M generation—utilize the highly refined GEOID18 model for CONUS, Puerto Rico, and the U.S. Virgin Islands.31 GEOID18 represents a vastly improved, mathematically complex surface fit to the NAVD88 datum, derived by incorporating tens of thousands of new GPS-on-Benchmark observations. This refinement reduced the overall standard deviation of residuals across CONUS from approximately 1.7 cm (under GEOID12B) to an incredibly precise 1.27 cm.41

Because GEOID18 is geographically restricted to CONUS and the Caribbean territories, elevation data generated for Alaska continues to rely on GEOID12B for transformations to NAVD88, or utilizes heavily localized regional vertical datums depending on the specific IfSAR collection parameters.41 Automated systems must therefore implement spatial conditional logic that applies different vertical shift grids based on the latitude and longitude of the ingestion request.

### 5.3 Programmatic Blending of 3DEP (NAVD88) with Copernicus GLO-30 (EGM2008)

Global digital elevation models, such as the Copernicus GLO-30 DEM or SRTM, do not utilize terrestrial benchmark networks. Instead, they universally utilize the Earth Gravitational Model of 2008 (EGM2008) as their vertical reference datum.40 EGM2008 is a purely gravimetric global geoid model derived from satellite gravity missions, lacking the localized adjustments to leveled terrestrial benchmarks that define the hybrid NAVD88 system.40

To programmatically blend 3DEP tiles (NAVD88) with Copernicus tiles (EGM2008) at a national boundary or coastline, the automated software cannot simply mosaic the arrays together. It must execute a computationally rigorous vertical datum transformation to align the equipotential surfaces.38

Because both systems can be mathematically related back to a highly similar common reference ellipsoid (WGS84 and NAD83 differ horizontally by a negligible amount for mid-resolution terrain modeling, though strictly NAD83 is tied to the drift of the North American tectonic plate), the programmatic workflow to convert a 3DEP pixel value to an EGM2008 compatible value is derived as follows:

- **Extract Ellipsoidal Height from 3DEP:** Add the GEOID18 undulation back to the published NAVD88 orthometric height to recreate the GNSS ellipsoidal height.

- **Calculate EGM2008 Orthometric Height:** Subtract the EGM2008 gravimetric geoid undulation from the recreated ellipsoidal height.

- **Combined System Transformation Equation:**

```
H_egm2008 = H_navd88 + N_geoid18(lat,lon) - N_egm2008(lat,lon)

Or equivalently:
H_egm2008 = H_navd88 + ΔN(lat,lon)

where ΔN = N_geoid18 - N_egm2008 at the given coordinate
```

In programmatic implementations—such as utilizing the PROJ library wrapped by GDAL Virtual Rasters (VRT)—this equation is not executed cell-by-cell in application code. Instead, it is executed via pre-compiled transformation grid files (e.g., .gtx or .tif shift grids) managed internally by the software's CRS pipeline.38 The software evaluates the precise offset  at the given latitude and longitude and applies this arithmetic shift to the raster matrix in memory prior to allowing the arrays to blend.38

| **Vertical Reference System** | **Geoid / Transformation Model** | **Primary Application** | **Geodetic Nature** |
| --- | --- | --- | --- |
| **NAVD88 (CONUS)** | GEOID18 | 3DEP 1m, 1/3 arc-second | Hybrid Orthometric (Benchmarks) |
| **NAVD88 (Alaska)** | GEOID12B | 3DEP 5m, 2 arc-second | Hybrid Orthometric |
| **Global Standards** | EGM2008 | Copernicus GLO-30, SRTM | Gravimetric (Satellite) |

## 6. Coverage Milestones, Quality Levels, and the Future Acquisition Strategy

The availability and resolution of 3DEP data are entirely governed by the progressive, federally managed acquisition of lidar across the continent, which directly dictates the fallback logic automated systems must employ. The USGS classifies elevation data into strict Quality Levels (QL), ranging from QL0 (highest fidelity) to QL5 (lowest fidelity).

### 6.1 The 2025 CONUS Coverage Baseline

The 3DEP initiative has successfully achieved its monumental baseline objective. At the conclusion of fiscal year 2024, 3DEP terrestrial elevation data was officially available or in progress for 98.3% of the Nation.47 By the close of fiscal year 2025, an astonishing 99% of the conterminous United States has baseline data—defined as Quality Level 2 or better lidar, which directly equates to the 1-meter DEM resolution—either fully available in the public domain or under active contract award.48

Therefore, for automated workflows executing in 2025 and 2026, 1-meter high-resolution coverage over CONUS is effectively ubiquitous.49 Software engines can expect 1-meter data requests to succeed with extreme reliability, with rare exceptions limited to highly restricted military airspace or extraordinarily remote, un-flown tribal lands.49

### 6.2 The Unique Geodetic Challenge of Alaska

Due to extreme weather patterns, impossibly remote and rugged terrain, and persistent dense cloud cover, widespread airborne lidar collection across the entirety of Alaska is logistically and financially impractical. Consequently, the 3DEP baseline coverage for Alaska relies almost entirely on Interferometric Synthetic Aperture Radar (IfSAR) technologies.48

This distinct acquisition method yields a standard product of 5-meter resolution DEMs, which corresponds to the 3DEP Quality Level 5 (QL5) specification.1 This data is seamlessly aggregated into a 2 arc-second (approximately 60-meter) continuous DEM layer.35 Automated systems operating over Alaskan coordinates must be programmatically instructed to adjust their highest-resolution expectations from 1-meter down to 5-meters, triggering specialized ingestion logic.1

### 6.3 Hawaii, U.S. Territories, and the Next Generation 3DEP

Hawaii, Puerto Rico, and other U.S. territories are fully integrated into the seamless 1/3 arc-second dataset and are actively being mapped under the S1M lidar initiative.4 By 2025 and 2026, QL2 lidar coverage is heavily saturated in these insular regions, although the temporal frequency of updates may lag significantly behind the rapid refresh rates seen in CONUS.47

Looking to the future, the USGS has officially launched the "Next Generation 3DEP" under the broader umbrella of the 3D National Topography Model (3DNTM).47 Having completed the national baseline, this new program shifts focus toward temporal refresh cycles. The strategy aims to mandate repeat coverages at even higher Quality Levels (specifically QL1 or QL0, which feature an aggregate nominal pulse density of over 8 points per square meter and equate to a 0.5-meter DEM cell size) with an aggressive update cycle targeting 4 to 5 years for rapidly changing topographies.47

| **Quality Level** | **Vertical Accuracy (RMSEz)** | **Aggregate Pulse Density** | **DEM Cell Size** | **Primary Technology** |
| --- | --- | --- | --- | --- |
| **QL0** | 5.0 cm | 8.0 pts/m² | 0.5 meters | Next-Gen Lidar 47 |
| **QL1** | 10.0 cm | 8.0 pts/m² | 0.5 meters | Next-Gen Lidar 47 |
| **QL2** | 10.0 cm | 2.0 pts/m² | 1.0 meters | Standard 3DEP Baseline 47 |
| **QL5** | 100.0 cm | N/A | 5.0 meters | IfSAR (Alaska) 47 |

## 7. Practical Programmatic Workflow Execution in a Rust Environment

For a headless, high-concurrency Rust engine designed for tasks such as programmatic aerial survey planning or autonomous flight management (e.g., the hypothetical "IronTrack" engine architecture), interacting natively with 3DEP requires orchestrating asynchronous HTTP network calls, deeply nested STAC JSON parsing, and cloud-native raster windowing.5

The following outlines the precise, robust logical workflow for such a Rust application to dynamically query, fall back, and geometrically shift elevation data.

### 7.1 Step A: Querying for High-Resolution Coverage via STAC

Instead of traversing archaic FTP directories or building custom string parsers for the legacy TNM /products API, the modern Rust application will interact exclusively with the Microsoft Planetary Computer STAC endpoint using a native Rust client ecosystem.11

- **Client Initialization:** The engine instantiates a STAC client utilizing the stac-client crate.51 This library relies on the tokio asynchronous runtime for non-blocking I/O and reqwest for executing rapid HTTP protocols.51 Crucially, the client is configured with exponential backoff and jitter to ensure resilience against transient cloud API throttling.51

- **Spatial Envelope Definition:** The application defines the target operational survey area as a strict spatial bounding box formatted as an array [min_lon, min_lat, max_lon, max_lat].22

- **Query Execution:** The software dynamically constructs a STAC search query directed at the 3dep-lidar-dtm-native collection (targeting 1-meter COGs) or the broader 3dep-seamless collection.23 The Rust engine awaits the execution of this search, parsing the response asynchronously into strongly-typed struct models.51

- **Response Parsing:** The payload returned is a fully compliant GeoJSON ItemCollection. The Rust engine inspects the collection size; if it contains valid items, the spatial footprint of the 1-meter data is verified to exist for the requested operational envelope, and the system proceeds to extraction.

### 7.2 Step B: Fallback Logic Implementation

Enterprise resilience demands the graceful degradation of resolution.5 If the initial STAC query for 1-meter data returns an empty subset—or if the flight envelope crosses sovereign borders into Mexico, Canada, or deep unmapped terrain—the engine must automatically initiate a rigid fallback cascade.

- **Primary Fallback (1/3 arc-second):** The engine re-issues a secondary STAC search against the 3dep-seamless collection.23 Because 1/3 arc-second data is ubiquitous across the entirety of CONUS and its territories, this query will definitively return valid asset items. The application parses the Item metadata attributes to verify the resolution matches the ~10m specification.

- **Secondary Fallback (Copernicus GLO-30):** If the bounding box lies entirely outside U.S. territory (where the 3DEP mandate ceases), the engine executes a tertiary STAC query to the cop-dem-glo-30 collection on the Planetary Computer to retrieve the 30-meter global fallback layer.28

### 7.3 Step C: Windowed I/O Extraction and On-the-Fly Datum Blending

Once the appropriate STAC Items are securely acquired and the fallback hierarchy is resolved, the Rust application must extract the physical pixel data. Because the STAC Items contain direct Uniform Resource Identifier (URI) asset links pointing to the COGs hosted on Azure or AWS 23, the software never attempts a full file download.

- **Virtual File System (VSI) Mounting:** The engine passes the cloud asset URL (e.g., https://prd-tnm.s3.amazonaws.com/...) to the GDAL bindings in Rust (gdal-rs), explicitly utilizing the /vsicurl/ virtual file system handler.22

- **Range Read Execution:** Behind the scenes, gdal-rs issues a highly targeted HTTP GET Range request to fetch only the TIFF Image File Directory (IFD) header, algorithmically determines the internal byte layout, and pulls strictly the specific compressed pixel blocks that physically overlap the requested bounding box.6

- **Coordinate Transformation and VRT Construction:** The engine sets up an in-memory Virtual Raster (VRT). When the system detects the necessity to blend 3DEP (NAVD88) with Copernicus (EGM2008), the VRT is dynamically configured with a PROJ transformation pipeline. By linking to local geoid grid files (such as g2018u0.gtx for GEOID18), the pipeline applies the  arithmetic shift dynamically as the binary pixels are read into RAM, ensuring physical consistency without modifying the source files.38

- **Reprojection:** The engine seamlessly reprojects the disparate tiles—converting from angular Latitude/Longitude for the 1/3 arc-second data, or Albers EPSG:6350 for the S1M data—into the highly localized working Coordinate Reference System (CRS) required by the flight planner for accurate photogrammetric Euclidean math (e.g., UTM Zone 10N).5

- **Persistence:** The final, continuous, geodetically corrected elevation matrix is cached locally or persisted into a GeoPackage elevation extension layer, ensuring immediate, offline access by any graphical frontend component.5

## 8. The Open-Source Geospatial Ecosystem and Implementations

The open-source geospatial community provides an expansive suite of robust tools designed to accelerate this programmatic ingestion, completely bypassing the need for systems engineers to write raw HTTP parsing logic or complex geodetic equations from scratch.

### 8.1 The Rust Ecosystem (rustac and stac-client)

Rust’s strict emphasis on memory safety, zero-cost abstractions, and fearless concurrency has catalyzed the growth of a specialized geospatial ecosystem directly applicable to high-performance 3DEP integration.5

The stac-utils organization maintains rustac, a highly comprehensive monorepo containing the core data structures necessary for interacting with the STAC API specification.11 The foundational stac crate provides strongly-typed Rust structs for Catalogs, Collections, and Items, and heavily leverages the serde_json framework for blisteringly fast serialization and deserialization of the complex API payloads.53

Built upon this foundation, the stac-client crate provides an asynchronous client architecture.51 Inspired by Python's popular tooling, stac-client provides a fluent builder pattern for constructing intricate STAC spatial searches, manages native pagination handling, and integrates pluggable authentication modules.51 This crate acts as the ideal conduit for querying the Microsoft Planetary Computer or the USGS native STAC endpoints without blocking the main execution thread. Furthermore, the gdal crate provides safe Rust bindings to the C++ Geospatial Data Abstraction Library (GDAL), which remains the most battle-tested mechanism in the industry for executing the /vsicurl/ reads on 3DEP COGs and managing the complex PROJ vertical datum transformations.22

### 8.2 Python and C++ Ecosystem Paradigms

For ecosystems adjacent to Rust, standard tooling heavily relies on GDAL and its high-level wrappers. In the Python ecosystem, rasterio implements highly Pythonic, idiomatic bindings for GDAL.54 When combined with pystac-client, data scientists and backend engineers can effortlessly discover 3DEP STAC items and stream them directly into memory arrays via rasterio.open(), which inherently handles the necessary COG HTTP Range requests under the hood.

For environments requiring direct manipulation of the raw lidar point clouds hosted in the usgs-lidar S3 bucket, the Point Data Abstraction Library (PDAL) provides extraordinarily powerful C++ filtering mechanisms. PDAL pipelines—specifically leveraging modules like pdal.filters.smrf for ground classification and pdal.filters.hag_nn for normalization—are extensively used by cloud providers to generate derivative bare-earth DEMs or Height Above Ground (HAG) rasters dynamically from the USGS LAZ data on the fly, demonstrating the immense computational potential of the modernized 3DEP cloud architecture.55

## 9. Conclusion and Strategic Outlook

The USGS 3D Elevation Program has successfully executed a paradigm-shifting transition from a fragmented repository of localized, static files into a deeply modernized, cloud-native geospatial architecture. For automated software systems, the migration of all primary DEM products to Cloud Optimized GeoTIFFs hosted on public AWS S3 buckets—combined with comprehensive indexing via STAC-compliant APIs—allows for unprecedented speed, memory efficiency, and deterministic reliability in programmatic data ingestion.

Systems developers constructing autonomous platforms must carefully navigate a tiered resolution structure where 1-meter data offers the highest terrestrial fidelity, but the 1/3 arc-second data provides universally reliable fallback coverage across the entirety of the conterminous United States. The recent introduction of the Seamless 1-Meter (S1M) product radically simplifies ingestion algorithms by replacing disparate, UTM-based project tiles with standardized 10x10 km tiles aligned to a singular, projected Albers Equal Area coordinate system.

However, the complexities of geodetic science cannot be abstracted away by cloud formats; integrating domestic 3DEP data with global satellite products demands rigorous programmatic transformation between the orthometric, benchmark-tied NAVD88/GEOID18 system and the purely gravimetric EGM2008 system. Utilizing modern programming ecosystems like Rust's stac-client and gdal-rs provides the robust, memory-safe, and highly concurrent tooling necessary to automate these intricate spatial queries, execute complex fallback logics, and process on-the-fly datum adjustments, ultimately ensuring flawless terrain awareness in next-generation autonomous applications.

#### Works cited

- 5 Meter Alaska Digital Elevation Models (DEMs) - USGS National Map 3DEP Downloadable Data Collection, accessed March 23, 2026, [https://data.usgs.gov/datacatalog/data/USGS:e250fffe-ed32-4627-a3e6-9474b6dc6f0b](https://data.usgs.gov/datacatalog/data/USGS:e250fffe-ed32-4627-a3e6-9474b6dc6f0b)

- About 3DEP Products & Services | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/3d-elevation-program/about-3dep-products-services](https://www.usgs.gov/3d-elevation-program/about-3dep-products-services)

- Data Catalog - OpenTopography, accessed March 23, 2026, [https://portal.opentopography.org/dataCatalog](https://portal.opentopography.org/dataCatalog)

- USGS 1/3 arc-second Digital Elevation Model - OpenTopography, accessed March 23, 2026, [https://portal.opentopography.org/raster?opentopoID=OTNED.012021.4269.1](https://portal.opentopography.org/raster?opentopoID=OTNED.012021.4269.1)

- Irontrack

- What are Cloud Optimized GeoTIFFs (COGs)? | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/faqs/what-are-cloud-optimized-geotiffs-cogs](https://www.usgs.gov/faqs/what-are-cloud-optimized-geotiffs-cogs)

- What types of elevation datasets are available, what formats do they come in, and where can I download them? | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/faqs/what-types-elevation-datasets-are-available-what-formats-do-they-come-and-where-can-i-download](https://www.usgs.gov/faqs/what-types-elevation-datasets-are-available-what-formats-do-they-come-and-where-can-i-download)

- USGS 3DEP Lidar Collection | Planetary Computer - Microsoft, accessed March 23, 2026, [https://planetarycomputer.microsoft.com/dataset/group/3dep-lidar](https://planetarycomputer.microsoft.com/dataset/group/3dep-lidar)

- USGS 3DEP LiDAR Point Clouds - Registry of Open Data on AWS, accessed March 23, 2026, [https://registry.opendata.aws/usgs-lidar/](https://registry.opendata.aws/usgs-lidar/)

- SpatioTemporal Asset Catalog (STAC) | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/landsat-missions/spatiotemporal-asset-catalog-stac](https://www.usgs.gov/landsat-missions/spatiotemporal-asset-catalog-stac)

- GitHub - stac-utils/rustac: The power of Rust for the STAC ecosystem, accessed March 23, 2026, [https://github.com/stac-utils/rustac](https://github.com/stac-utils/rustac)

- TNM Access API Documentation - The National Map, accessed March 23, 2026, [https://tnmaccess.nationalmap.gov/api/v1/docs](https://tnmaccess.nationalmap.gov/api/v1/docs)

- Is there an API for accessing The National Map data? | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/faqs/there-api-accessing-national-map-data](https://www.usgs.gov/faqs/there-api-accessing-national-map-data)

- Downloading NAIP Data via the TNM Access API - GIS StackExchange, accessed March 23, 2026, [https://gis.stackexchange.com/questions/490355/downloading-naip-data-via-the-tnm-access-api](https://gis.stackexchange.com/questions/490355/downloading-naip-data-via-the-tnm-access-api)

- Dispatches from the Geospatial Data World | Page 5 - At These Coordinates, accessed March 23, 2026, [https://atcoordinates.info/page/5/](https://atcoordinates.info/page/5/)

- geospatial-data-catalogs/aws_stac_catalogs.tsv at master - GitHub, accessed March 23, 2026, [https://github.com/giswqs/geospatial-data-catalogs/blob/master/aws_stac_catalogs.tsv](https://github.com/giswqs/geospatial-data-catalogs/blob/master/aws_stac_catalogs.tsv)

- 2020 USGS Lidar DEM: Savannah Pee Dee, SC | InPort - NOAA Fisheries, accessed March 23, 2026, [https://www.fisheries.noaa.gov/inport/item/65959](https://www.fisheries.noaa.gov/inport/item/65959)

- Good ol' Rockyweb - Digital Coast GeoZone, accessed March 23, 2026, [https://geozoneblog.wordpress.com/2021/11/17/good-ol-rockyweb/](https://geozoneblog.wordpress.com/2021/11/17/good-ol-rockyweb/)

- 1 meter Digital Elevation Models (DEMs) - USGS National Map 3DEP Downloadable Data Collection, accessed March 23, 2026, [https://data.usgs.gov/datacatalog/data/USGS:77ae0551-c61e-4979-aedd-d797abdcde0e](https://data.usgs.gov/datacatalog/data/USGS:77ae0551-c61e-4979-aedd-d797abdcde0e)

- AWS Marketplace: USGS 3DEP LiDAR Point Clouds - Amazon.com, accessed March 23, 2026, [https://aws.amazon.com/marketplace/pp/prodview-647e3hzk3b3jc](https://aws.amazon.com/marketplace/pp/prodview-647e3hzk3b3jc)

- Lidar Point Cloud in AWS - Requester Pays Instructions, accessed March 23, 2026, [https://d9-wret.s3.us-west-2.amazonaws.com/assets/palladium/production/s3fs-public/media/files/Lidar%20Point%20Cloud%20in%20AWS%20-%20Requester%20Pays%20Instructions_v2.pdf](https://d9-wret.s3.us-west-2.amazonaws.com/assets/palladium/production/s3fs-public/media/files/Lidar%20Point%20Cloud%20in%20AWS%20-%20Requester%20Pays%20Instructions_v2.pdf)

- Download data from a STAC API using R, rstac, and GDAL - SpatioTemporal Asset Catalogs, accessed March 23, 2026, [https://stacspec.org/en/tutorials/1-download-data-using-r/](https://stacspec.org/en/tutorials/1-download-data-using-r/)

- USGS 3DEP Seamless DEMs | Planetary Computer - Microsoft, accessed March 23, 2026, [https://planetarycomputer.microsoft.com/dataset/3dep-seamless](https://planetarycomputer.microsoft.com/dataset/3dep-seamless)

- USGS 3DEP Lidar Digital Terrain Model (Native) | Planetary Computer - Microsoft, accessed March 23, 2026, [https://planetarycomputer.microsoft.com/dataset/3dep-lidar-dtm-native](https://planetarycomputer.microsoft.com/dataset/3dep-lidar-dtm-native)

- USGS 3DEP Lidar Point Cloud (3dep-lidar-copc) - STAC Index, accessed March 23, 2026, [https://stacindex.org/catalogs/usgs-3dep-lidar-point-cloud-copc](https://stacindex.org/catalogs/usgs-3dep-lidar-point-cloud-copc)

- API and Web Services | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/sciencebase-instructions-and-documentation/api-and-web-services](https://www.usgs.gov/sciencebase-instructions-and-documentation/api-and-web-services)

- API access to USGS 3DEP rasters now available - OpenTopography, accessed March 23, 2026, [https://opentopography.org/news/api-access-usgs-3dep-rasters-now-available](https://opentopography.org/news/api-access-usgs-3dep-rasters-now-available)

- OpenTopography for Developers, accessed March 23, 2026, [https://opentopography.org/developers](https://opentopography.org/developers)

- The Accuracy and Consistency of 3D Elevation Program Data: A Systematic Analysis - MDPI, accessed March 23, 2026, [https://www.mdpi.com/2072-4292/14/4/940](https://www.mdpi.com/2072-4292/14/4/940)

- New Product from the 3D Elevation Program: Seamless 1 Meter ..., accessed March 23, 2026, [https://www.usgs.gov/3d-elevation-program/new-product-3d-elevation-program-seamless-1-meter-digital-elevation-model-s1m](https://www.usgs.gov/3d-elevation-program/new-product-3d-elevation-program-seamless-1-meter-digital-elevation-model-s1m)

- Seamless 1 meter Digital Elevation Models (DEMs) - USGS National Map 3DEP Downloadable Data Collection, accessed March 23, 2026, [https://data.usgs.gov/datacatalog/data/USGS:4f34caac-f28f-4ea0-8d82-eafb2b8f9a5d](https://data.usgs.gov/datacatalog/data/USGS:4f34caac-f28f-4ea0-8d82-eafb2b8f9a5d)

- USGS 3DEP 1m National Map | Earth Engine Data Catalog - Google for Developers, accessed March 23, 2026, [https://developers.google.com/earth-engine/datasets/catalog/USGS_3DEP_1m](https://developers.google.com/earth-engine/datasets/catalog/USGS_3DEP_1m)

- 3DEPElevationIndex (MapServer) - The National Map, accessed March 23, 2026, [https://index.nationalmap.gov/arcgis/rest/services/3DEPElevationIndex/MapServer](https://index.nationalmap.gov/arcgis/rest/services/3DEPElevationIndex/MapServer)

- USGS 1 arc-second Digital Elevation Model - OpenTopography, accessed March 23, 2026, [https://portal.opentopography.org/raster?opentopoID=OTNED.012021.4269.2](https://portal.opentopography.org/raster?opentopoID=OTNED.012021.4269.2)

- Alaska 2 Arc-second Digital Elevation Models (DEMs) - USGS National Map 3DEP Downloadable Data Collection, accessed March 23, 2026, [https://data.usgs.gov/datacatalog/data/USGS:4bd95204-7a29-4bd4-acce-00551ecaf47a](https://data.usgs.gov/datacatalog/data/USGS:4bd95204-7a29-4bd4-acce-00551ecaf47a)

- Why are two different file naming conventions used in distribution of 3D Elevation Program (3DEP) DEM products? | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/faqs/why-are-two-different-file-naming-conventions-used-distribution-3d-elevation-program-3dep-dem](https://www.usgs.gov/faqs/why-are-two-different-file-naming-conventions-used-distribution-3d-elevation-program-3dep-dem)

- Ranking of 10 Global One-Arc-Second DEMs Reveals Limitations in Terrain Morphology Representation - Air Iuav, accessed March 23, 2026, [https://air.iuav.it/retrieve/8a972e00-1400-4769-8201-30ae35f30f79/remotesensing-16-03273-v2.pdf](https://air.iuav.it/retrieve/8a972e00-1400-4769-8201-30ae35f30f79/remotesensing-16-03273-v2.pdf)

- Converting from orthometric to ellipsoidal heights—ArcGIS Pro | Documentation, accessed March 23, 2026, [https://pro.arcgis.com/en/pro-app/3.4/help/data/imagery/converting-from-orthometric-to-ellipsoidal-heights-pro.htm](https://pro.arcgis.com/en/pro-app/3.4/help/data/imagery/converting-from-orthometric-to-ellipsoidal-heights-pro.htm)

- Vertical accuracy of the USGS 3DEP program data: study cases in Fresno County and in Davis, California - SciELO, accessed March 23, 2026, [https://www.scielo.br/j/bcg/a/pc9wjzfq6zzWN5kdfFgcPPr/?format=pdf&lang=en](https://www.scielo.br/j/bcg/a/pc9wjzfq6zzWN5kdfFgcPPr/?format=pdf&lang=en)

- Find Ellipsoidal Height from Orthometric Height - MATLAB & Simulink - MathWorks, accessed March 23, 2026, [https://www.mathworks.com/help/map/ellipsoid-geoid-and-orthometric-height.html](https://www.mathworks.com/help/map/ellipsoid-geoid-and-orthometric-height.html)

- GEOID18 Technical Details - National Geodetic Survey (NGS) - NOAA, accessed March 23, 2026, [https://geodesy.noaa.gov/GEOID/GEOID18/geoid18_tech_details.shtml](https://geodesy.noaa.gov/GEOID/GEOID18/geoid18_tech_details.shtml)

- GEOID Model Homepage - National Geodetic Survey (NGS), accessed March 23, 2026, [https://geodesy.noaa.gov/GEOID/](https://geodesy.noaa.gov/GEOID/)

- GEOID99 and G99SSS: 1-arc-minute geoid models for the United States - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/226769393_GEOID99_and_G99SSS_1-arc-minute_geoid_models_for_the_United_States](https://www.researchgate.net/publication/226769393_GEOID99_and_G99SSS_1-arc-minute_geoid_models_for_the_United_States)

- A tutorial on datums - NOAA/NOS's VDatum, accessed March 23, 2026, [https://vdatum.noaa.gov/docs/datums.html](https://vdatum.noaa.gov/docs/datums.html)

- Frequently Asked Questions for USGG2009/GEOID09/USDOV2009/DEFLEC09, accessed March 23, 2026, [https://geodesy.noaa.gov/GEOID/USGG2009/faq_2009.shtml](https://geodesy.noaa.gov/GEOID/USGG2009/faq_2009.shtml)

- How To: Project Vertical Datum for Elevation Data Using the ProjectZ Tool in ArcGIS Pro, accessed March 23, 2026, [https://support.esri.com/en-us/knowledge-base/how-to-project-vertical-datum-for-elevation-data-using--000026499](https://support.esri.com/en-us/knowledge-base/how-to-project-vertical-datum-for-elevation-data-using--000026499)

- The 3D National Topography Model Call for Action—Part 2: The Next Generation 3D Elevation Program - USGS Publications Warehouse, accessed March 23, 2026, [https://pubs.usgs.gov/publication/cir1553/full](https://pubs.usgs.gov/publication/cir1553/full)

- What is 3DEP? | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/3d-elevation-program/what-3dep](https://www.usgs.gov/3d-elevation-program/what-3dep)

- FY25 Status of 3DEP Quality Data | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/media/images/fy25-status-3dep-quality-data](https://www.usgs.gov/media/images/fy25-status-3dep-quality-data)

- FY26 3DEP Data Collaboration Announcement (DCA) | U.S. Geological Survey - USGS.gov, accessed March 23, 2026, [https://www.usgs.gov/3d-national-topography-model/fy26-3dep-data-collaboration-announcement-dca](https://www.usgs.gov/3d-national-topography-model/fy26-3dep-data-collaboration-announcement-dca)

- stac-client - crates.io: Rust Package Registry, accessed March 23, 2026, [https://crates.io/crates/stac-client](https://crates.io/crates/stac-client)

- rustac, accessed March 23, 2026, [https://stac-utils.github.io/rustac/](https://stac-utils.github.io/rustac/)

- stac - Rust - Docs.rs, accessed March 23, 2026, [https://docs.rs/stac](https://docs.rs/stac)

- Visualizing MAAP STAC Dataset with MosaicJSON, accessed March 23, 2026, [https://docs.maap-project.org/en/ogc/technical_tutorials/visualization/visualizing_mosaicjson.html](https://docs.maap-project.org/en/ogc/technical_tutorials/visualization/visualizing_mosaicjson.html)

- USGS 3DEP Lidar Height above Ground | Planetary Computer - Microsoft, accessed March 23, 2026, [https://planetarycomputer.microsoft.com/dataset/3dep-lidar-hag](https://planetarycomputer.microsoft.com/dataset/3dep-lidar-hag)