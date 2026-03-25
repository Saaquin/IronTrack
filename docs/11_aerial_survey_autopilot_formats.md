# Commercial Aerial Survey Autopilot Systems: A Comprehensive Technical Survey of Waypoint and Mission File Formats

## The Evolution of Automated Aerial Mapping and Interoperability

The rapid maturation of unmanned aerial systems (UAS) and hybrid airborne sensor platforms has fundamentally altered the landscape of photogrammetry, remote sensing, and LiDAR point cloud generation. The success of these aerial mapping endeavors relies absolutely on the precision of the flight management system (FMS) and the onboard autopilot. These systems interpret complex, multi-waypoint trajectories defined within specific mission file formats, translating abstract geographic intent into physical electromechanical actuation.

For commercial applications such as high-resolution orthomosaicking, oblique urban modeling, and infrastructure inspection, the precision of the generated flight plan directly dictates the geometric fidelity of the resulting datasets. A fragmented ecosystem of proprietary, open-source, and quasi-standardized file formats currently governs the interoperability between mission planning software and autopilot hardware. These formats must encode spatial coordinates (latitude, longitude, altitude) alongside granular hardware metadata, including gimbal pitch, mechanical shutter synchronization, yaw orientation, terrain-following rules, and dynamic camera triggering intervals.

This analysis provides an exhaustive technical deconstruction of the mission file formats utilized by the industry's premier FMS and autopilot ecosystems: ArduPilot/PX4 (QGroundControl JSON and MAVLink protocols), DJI Enterprise (WPML/KMZ), senseFly/AgEagle (eMotion XML), and Leica Geosystems (HxMap/FlightPro XML/PMP).

### ASPRS Positional Accuracy Standards and Flight Plan Optimization

The driving force behind the rigid specification of mission file formats is the necessity to comply with global mapping standards, most notably the American Society for Photogrammetry and Remote Sensing (ASPRS) Positional Accuracy Standards for Digital Geospatial Data.1

Automated mission planning is fundamentally an exercise in geometric optimization. The photogrammetric pipeline requires strict adherence to predefined Ground Sample Distances (GSD) and stereoscopic overlap percentages to resolve accurate 3D Digital Twins.3 The ASPRS standards outline rigorous verification frameworks for positional accuracy, logical consistency, and completeness, utilizing statistical measures such as the Root Mean Square Error (RMSE) and Circular Error Probability (CEP 95).5

| **Accuracy Level** | **Largest Scale** | **RMSE (Horizontal)** | **CEP 95 (Horizontal)** |
| --- | --- | --- | --- |
| 1 | 1:50 | 0.01 m | 0.03 m |
| 2 | 1:100 | 0.03 m | 0.08 m |
| 4 | 1:500 | 0.13 m | 0.32 m |
| 6 | 1:1,250 | 0.30 m | 0.74 m |
| 8 | 1:5,000 | 1.25 m | 3.06 m |

Table 1: Horizontal accuracy thresholds adapted from standard digital mapping frameworks comparable to ASPRS specifications.6

To achieve Level 1 or 2 accuracy, the flight plan must ensure that camera trigger intervals and flight line cross-track spacing never deviate from the mathematical optimum. The FMS files must precisely dictate to the autopilot when and where to trigger the sensor to maintain required data quality elements. The recent ASPRS Addendum VI specifically formalizes the framework for oblique imagery acquisition, demanding that FMS mission files possess the capacity to execute multi-angle image capture via synchronized multi-camera arrays.2

## Geodesy, Altitude Reference Systems, and Autopilot Corrections

A defining characteristic of an advanced commercial autopilot system is its treatment of altitude datums. Translating a planned mission altitude into an executed 3D flight trajectory requires a rigorous and unambiguous definition of the vertical coordinate reference system (CRS). Inconsistencies or misunderstandings in altitude definitions lead to severe GSD variations, loss of side-lap, or catastrophic Controlled Flight Into Terrain (CFIT).

Mission file formats generally encode altitude using one of three primary reference frameworks:

- **Ellipsoidal Height:** The height above a mathematical reference ellipsoid (predominantly WGS84). Global Navigation Satellite Systems (GNSS), including RTK and PPK receivers, natively calculate and output ellipsoidal heights.7

- **Orthometric Height / Mean Sea Level (MSL):** Altitude referenced to a gravimetric geoid model, most commonly EGM96 or EGM2008. Converting ellipsoidal height to orthometric height requires the application of a geoid undulation model.

- **Above Ground Level (AGL) / Terrain Following:** The altitude maintained relative to the underlying Digital Elevation Model (DEM) or Digital Terrain Model (DTM).

### Geoid Correction and Terrain Handling Mechanics

The responsibility for performing geoid corrections and terrain offset calculations varies significantly across FMS architectures.

In closed-loop ecosystems such as DJI, the DJI Pilot 2 software and FlightHub 2 utilize EGM96 as the default global elevation model. The FMS offloads the computational burden to the cloud or the ground station tablet. When an operator plans an AGL mission, the planning software queries a background DEM, translates the requested AGL offsets into EGM96 orthometric heights, and compiles these absolute values into the flight file. Consequently, the autopilot executes predefined absolute altitudes rather than computing terrain offsets in real-time.8

Conversely, the open-source ArduPilot ecosystem supports dynamic onboard terrain following. The ground control station (GCS) uploads waypoints utilizing a specific terrain-altitude reference frame. During flight, the ArduPilot firmware actively requests terrain tiles from the GCS via the MAVLink TERRAIN_REQUEST message.10 Once the GCS replies with TERRAIN_DATA, the autopilot caches the Shuttle Radar Topography Mission (SRTM) data to its onboard SD card.11

The autopilot maintains an active grid of approximately 7 km by 8 km in memory, governed by the TERRAIN_SPACING parameter (defaulting to 100 meters).11 This allows the onboard flight controller to dynamically calculate the required ascent and descent profiles to maintain the requested AGL. To prevent the aircraft from flying into steep inclines that exceed its maximum climb rate, ArduPilot employs a look-ahead algorithm, defined by the TERRAIN_LOOKAHD parameter, which generally defaults to checking 2000 meters ahead of the current trajectory.11

## ArduPilot and PX4 Ecosystem: QGroundControl and MAVLink Protocols

The ArduPilot and PX4 open-source flight stacks are universally adopted in both commercial surveying and academic research. Mission planning for these systems is predominantly handled by QGroundControl (QGC), which utilizes a specific JSON schema (.plan) for offline storage. This file is subsequently parsed, translated into discrete commands, and transmitted to the autopilot via the MAVLink microservice protocol.

### QGroundControl.plan JSON Schema Architecture

QGC .plan files encapsulate mission items, geometric geofences, and rally points within a single, highly structured JSON document. The decoupled nature of this plaintext format allows geospatial analysts to construct complex surveying paths using third-party GIS software, script the outputs into the JSON schema, and seamlessly import them into QGC for physical execution.13

The root JSON object mandates the following primary architectural fields:

| **JSON Key** | **Data Type** | **Structural Description** |
| --- | --- | --- |
| fileType | String | Must be explicitly defined as "Plan" to be parsed correctly by the QGC ingestion engine.13 |
| version | Integer | Specifies the version of the root plan file format; the current standard version is 1.13 |
| groundStation | String | Identifies the origin software that generated the file (e.g., "QGroundControl").13 |
| mission | Object | The primary payload container for all flight path logic and abstracted MAVLink commands.13 |
| geoFence | Object | (Optional) Contains nested arrays for circles and polygons to define absolute volumetric flight boundaries.13 |
| rallyPoints | Object | (Optional) Contains an array of safe coordinates used for emergency Return-to-Launch (RTL) maneuvers.13 |

#### The Mission Object and Item Arrays

The nested mission object is where the core photogrammetric flight planning algorithms reside. QGC splits mission instructions into two distinct ontological classes: SimpleItem, which maps directly on a 1:1 basis to a MAVLink command executable by the autopilot, and ComplexItem, which acts as a high-level metadata container for automated survey pattern generation.13

Mandatory fields within the mission object include:

| **JSON Key** | **Data Type** | **Structural Description** |
| --- | --- | --- |
| firmwareType | Integer | The MAV_AUTOPILOT enum value identifying the target firmware (e.g., 3 for ArduPilot, 12 for PX4).13 |
| vehicleType | Integer | The MAV_TYPE enum value identifying the airframe (e.g., 2 for Quadrotor, 1 for Fixed Wing).13 |
| globalPlanAltitudeMode | Integer | Determines the default vertical datum reference frame for the entire FMS plan.13 |
| cruiseSpeed | Float | The default forward horizontal velocity for fixed-wing or VTOL transitions between waypoints.13 |
| hoverSpeed | Float | The default forward horizontal velocity for multi-rotor vehicles.13 |
| plannedHomePosition | Array | A 3-element float array [latitude, longitude, altitude] used as the geometric origin point when the vehicle is disconnected from the GCS.13 |
| items | Array | The sequential, comma-separated list of SimpleItem and ComplexItem JSON objects.13 |

#### ComplexItem and CameraCalc Photogrammetric Metadata

When a ComplexItem is instantiated (e.g., complexItemType: "Survey", "CorridorScan", or "StructureScan"), the JSON schema includes a critically important CameraCalc object.13 This object defines the active sensor's physical geometry and desired photogrammetric overlap, enabling QGC to calculate the physical swath width and ground footprint dynamically.

The CameraCalc metadata fields include:

| **JSON Key** | **Data Type** | **Photogrammetric Description** |
| --- | --- | --- |
| CameraName | String | A string identifier matching a known camera profile within QGC, or "Manual" for custom entries.13 |
| FocalLength | Float | The true focal length of the camera lens, measured in millimeters.13 |
| SensorWidth | Float | The physical width of the digital sensor (CMOS/CCD), measured in millimeters.13 |
| SensorHeight | Float | The physical height of the digital sensor, measured in millimeters.13 |
| ImageWidth | Integer | The horizontal resolution of the image in pixels.13 |
| ImageHeight | Integer | The vertical resolution of the image in pixels.13 |
| FrontalOverlap | Float | The target percentage of forward/longitudinal image overlap along the flight line.13 |
| SideOverlap | Float | The target percentage of lateral/cross-track image overlap between adjacent flight lines.13 |
| Landscape | Boolean | Defines camera orientation relative to the airframe (true for landscape, false for portrait).13 |

It is critical to note that ComplexItem metadata is uniquely interpreted by the ground control station; the autopilot itself has no concept of a "Survey" or a "Corridor Scan". Before transmission to the drone, the FMS compiler flattens these high-level polygon grids into a sequential array of standard SimpleItem waypoints and camera triggers, stripping away the theoretical geometries.14 The JSON schema retains the complexItems field solely to allow round-tripping—enabling the user to re-open the file in QGC and adjust the polygon boundaries post-creation.14

### The MAVLink 2.0 Mission Transfer Protocol

While the .plan JSON file serves as the persistence medium on the local workstation, the actual transmission of the mission to the autopilot hardware occurs via the MAVLink protocol. MAVLink is a lightweight, binary telemetry protocol specifically designed for resource-constrained systems operating over high-latency, lossy radio frequency (RF) links.15 The introduction of MAVLink 2.0 brought critical enhancements for commercial surveying, most notably cryptographic packet signing (authentication) and the transition from the legacy MISSION_ITEM message to MISSION_ITEM_INT.17

#### Mathematical Implications: MISSION_ITEM_INT vs. MISSION_ITEM

In the MAVLink 1.0 specification, the MISSION_ITEM message (Message ID 39) encoded the X (latitude) and Y (longitude) global coordinates using IEEE 754 32-bit floating-point numbers (float). Due to the inherent limitations of floating-point mantissa precision, representing global WGS84 coordinates (which require highly specific decimal placements to resolve down to the centimeter) as 32-bit floats results in catastrophic spatial truncation. At the Earth's equator, this rounding error introduces a positional drift of up to 1.2 meters. While acceptable for basic hobbyist navigation, a 1.2-meter arbitrary drift completely invalidates centimeter-grade RTK/PPK photogrammetry and renders LiDAR swath alignment mathematically impossible.20

To rectify this, MAVLink 2.0 deprecated the use of MISSION_ITEM for spatial commands and mandated the use of MISSION_ITEM_INT (Message ID 73) for all flight plans.20 In the MISSION_ITEM_INT payload, the latitude and longitude are transmitted as 32-bit signed integers (int32_t), strictly scaled by a factor of 10⁷ (10,000,000).18

By transmitting coordinates as `lat_int = lat_deg × 10⁷`, the protocol ensures that a latitude of 47.3985099 is serialized over the telemetry link as the integer 473985099. This integer scaling guarantees absolute sub-centimeter positional fidelity across the entire globe, immune to floating-point truncation.18

#### The Point-to-Point Transfer Sequence

The MAVLink mission transfer operates on a point-to-point, guaranteed-delivery microservice architecture with integrated retransmission timeouts.16 The standard upload sequence operates as follows:

- **Initiation:** The GCS sends a MISSION_COUNT message to the target system and component ID, indicating the total number of sequential mission items to be uploaded. A timeout clock is initiated.20

- **Request:** The autopilot processes the count, allocates onboard memory, and responds with a MISSION_REQUEST_INT, explicitly asking for sequence index 0 (seq==0).20

- **Transmission:** The GCS transmits the MISSION_ITEM_INT payload for sequence 0.

- **Iteration:** The autopilot receives the item, validates the cyclic redundancy check (CRC) and signature, and then transmits a MISSION_REQUEST_INT for sequence 1. This request-response loop continues sequentially.

- **Termination:** Once the final item is successfully received and parsed, the autopilot responds with a MISSION_ACK message containing the enum MAV_MISSION_ACCEPTED, finalizing the upload.20

If an item is received out of sequence, the vehicle drops the out-of-sequence packet and re-requests the expected index. If the onboard storage is exceeded, the autopilot aborts the transfer and transmits a MISSION_ACK with MAV_MISSION_NO_SPACE.20

### Critical MAV_CMD Encodings for Aerial Surveying

Within the MISSION_ITEM_INT payload, the physical behavior of the drone is dictated by the command field, which corresponds to a specific MAV_CMD enumeration. Up to seven floating-point parameters (param1 through param7) accompany each command to provide executable context. For automated surveying, the following commands are strictly mandatory:

#### MAV_CMD_NAV_WAYPOINT (Command ID 16)

This is the fundamental 3D spatial navigation command.18 It commands the autopilot to transit to the specified coordinate.

| **Parameter** | **Data Type** | **Executable Description** |
| --- | --- | --- |
| Param 1 | Float | **Hold Time:** Time in seconds to loiter/hold upon reaching the waypoint (Max 65535).24 |
| Param 2 | Float | **Acceptance Radius:** The radial boundary in meters within which the FMS considers the waypoint successfully "reached".18 |
| Param 3 | Float | **Pass Radius:** Allows trajectory smoothing. 0 indicates the vehicle must pass directly through the coordinate. A value > 0 dictates a pass-by radius in meters (positive for clockwise orbit, negative for counter-clockwise).18 |
| Param 4 | Float | **Yaw Angle:** The desired vehicle yaw heading upon arrival. If set to NaN, the FMS calculates the yaw vector required to face the next sequential waypoint.24 |
| Param 5 (X) | Int32 | **Latitude:** Scaled as .23 |
| Param 6 (Y) | Int32 | **Longitude:** Scaled as .23 |
| Param 7 (Z) | Float | **Altitude:** Expressed in meters, evaluated relative to the specified MAV_FRAME.24 |

#### MAV_CMD_DO_CHANGE_SPEED (Command ID 178)

This command alters the target horizontal velocity. In photogrammetry, speed must be meticulously controlled to eliminate motion blur and rolling shutter distortion while ensuring the sensor's write speed can keep pace with the ground footprint.24

| **Parameter** | **Data Type** | **Executable Description** |
| --- | --- | --- |
| Param 1 | Float | **Speed Type:** 0 = Airspeed, 1 = Ground Speed, 2 = Climb Speed, 3 = Descent Speed.18 |
| Param 2 | Float | **Speed:** The target velocity in meters per second (m/s). A value of -1 indicates no change.18 |
| Param 3 | Float | **Throttle:** Target throttle percentage. Primarily utilized by fixed-wing platforms to override cruise logic; ignored by multirotors. -1 indicates no change.18 |
| Param 4 | Float | **Relative:** Boolean toggle. 0 = Absolute speed command, 1 = Relative to current speed.18 |

#### MAV_CMD_DO_SET_CAM_TRIGG_DIST (Command ID 206)

This command is arguably the most critical for mapping FMS operations, as it shifts the camera triggering logic from temporal (time-based) to spatial (distance-based). Because fixed-wing and multirotor drones experience fluctuating ground speeds due to variable headwinds, time-based triggering inevitably results in overlapping density variations. Activating spatial triggering guarantees mathematically consistent image overlap regardless of environmental velocity changes.25

| **Parameter** | **Data Type** | **Executable Description** |
| --- | --- | --- |
| Param 1 | Float | **Distance:** The ground distance in meters between consecutive camera triggers. Setting this parameter to 0 disables distance-based triggering.27 |
| Param 2 | Float | **Shutter Integration Time:** Used to command the hardware FMS to hold the PWM/GPIO shutter signal high for a specific duration (in milliseconds) to interface with varying mechanical sensor delays.27 |
| Param 3 | Float | **Trigger Toggle:** A boolean logic switch (1 = trigger the camera once immediately upon command execution, 0 = no immediate trigger).27 |

### Programmatic Terrain Following Activation in ArduPilot

Terrain following in the MAVLink protocol is not activated by a specific MAV_CMD; rather, it is executed by manipulating the coordinate reference frame of the MISSION_ITEM_INT command itself.

Every mission item contains a frame field. By setting the frame byte to MAV_FRAME_GLOBAL_TERRAIN_ALT_INT (Enumeration value 11), the altitude specified in param7 (Z) is distinctly interpreted by the autopilot as meters above the terrain model, rather than meters above the WGS84 ellipsoid or the home launch point.30

To successfully execute this programmatically, the GCS software must ensure that the vehicle's onboard TERRAIN_ENABLE and TERRAIN_FOLLOW parameters are both set to 1 prior to flight.11 When the autopilot parses a MAV_CMD_NAV_WAYPOINT utilizing frame 11, it automatically queries its internal cached SRTM database to calculate the required geopotential MSL altitude to satisfy the requested AGL offset, actively modulating its pitch and throttle vectors to trace the topography.11

## DJI Enterprise Ecosystem: WPML and KMZ Formats

DJI has aggressively standardized its enterprise flight planning architectures across platforms like the DJI RC Plus, DJI Pilot 2 application, DJI FlightHub 2 cloud software, and third-party API integrations. The mechanism for this standardization is the Waypoint Markup Language (WPML), deployed via an archived KMZ file structure.34

### WPML Package Structure and KML/WPML Hierarchy

A DJI KMZ file is not a singular document; it is a standard ZIP archive conforming to a rigid directory tree. This structure deliberately separates human-readable business logic from the low-level machine execution code 34:

waypoints_mission.kmz

└── wpmz

├── res/ # Auxiliary resources (e.g., custom DEM files, images)

├── template.kml # High-level business logic, geometry, and global definitions

└── waylines.wpml # Low-level, executable trajectory and coordinate arrays

This structural bifurcation mirrors QGroundControl's separation between ComplexItem and SimpleItem. The template.kml file defines the abstract mission parameters—such as the spatial boundaries of the survey polygon, target camera overlap percentages, global velocity limits, and survey grid angles.9

When a mission is generated within FlightHub 2 or DJI Pilot 2, the compilation engine calculates the exact 3D physical flight path required to execute the template. It then writes this path as a sequential, highly granular array of points and actions into waylines.wpml. Upon execution, the DJI flight controller strictly reads the waylines.wpml file, ignoring the abstract geometries of the template.8

### Mandatory Fields and Flight Parameters

Both KML and WPML files utilize a standard XML schema, heavily extended with DJI's proprietary <wpml:> namespace tags. Within the template.kml file, the <wpml:missionConfig> element is mandatory and dictates the global operational rules for the FMS.9

| **WPML Tag** | **Allowed Values / Enumeration** | **Operational Description** |
| --- | --- | --- |
| wpml:flyToWaylineMode | safely, pointToPoint | Dictates the approach vector from takeoff to the first survey waypoint. safely forces an immediate vertical ascent to the highest calculated altitude between the takeoff point and the first waypoint to definitively clear localized ground obstacles.9 |
| wpml:finishAction | goHome, noAction, autoLand, gotoFirstWaypoint | Dictates the absolute FMS action executed upon mission completion.9 |
| wpml:exitOnRCLost | goContinue, executeLostAction | Dictates failsafe logic in the event of radio link loss. Setting this to goContinue is essential for autonomous Beyond Visual Line of Sight (BVLOS) mapping operations.9 |
| wpml:takeOffSecurityHeight | Float (meters) | The initial vertical ascent altitude. The drone will climb to this exact altitude before horizontal pitch/roll vectors are engaged.9 |
| wpml:globalTransitionalSpeed | Float (m/s) | The default horizontal velocity utilized when the drone is transitioning between non-adjacent waylines or survey blocks.9 |
| wpml:payloadPositionIndex | Integer (0, 1, 2) | Identifies the specific physical gimbal mount port to address. For instance, 0 corresponds to the Main/Front-Left gimbal, 1 to Front-Right, and 2 to the Top mount.9 |
| wpml:executeHeightMode | WGS84, EGM96, RELATIVE | Defines the global altitude datum used by the FMS to parse the waypoint heights.8 |

### Altitude Datum and Coordinate Reference Systems in WPML

DJI FMS architecture handles altitude fundamentally differently from ArduPilot, heavily relying on pre-computation rather than onboard dynamic sensing. The FMS supports three height modes: WGS84 (ellipsoidal height), EGM96 (orthometric height/MSL), and RELATIVE (AGL relative to the takeoff point).8

When an operator plans a terrain-following mission in DJI FlightHub 2 using the aboveGroundLevel positioning type, the cloud software queries its global DEM database. It calculates the necessary absolute EGM96 orthometric altitude for every single waypoint along the path to maintain the requested AGL offset. It then compiles these static EGM96 values into the waylines.wpml file.9 Therefore, the DJI autopilot does not calculate terrain offsets in flight; it merely executes the explicit, static altitude values pre-compiled by the planning software.9

For high-precision RTK surveying missions, the wpml:positioningType must be explicitly set to RTKBaseStation or NetworkRTK. This designation forces the flight controller to deprioritize its onboard barometric altimeter and rely strictly on the Z-axis ellipsoidal height corrections provided by the RTCM data stream, which is crucial for achieving ASPRS Level 1 accuracy.

### Camera Trigger Encoding and Dynamic Intervals

DJI implements a highly sophisticated, nested XML structure for camera triggers using the <wpml:actionGroup> tag within the executable waylines.wpml file.37

Instead of attaching a trigger command to a single, localized waypoint, an actionGroup is mapped to an entire flight segment using an index range, defined by wpml:actionGroupStartIndex and wpml:actionGroupEndIndex. This decoupled architecture natively supports the execution of dynamic, variable-density camera intervals.37

Within the actionGroup, the wpml:actionTrigger tag defines the specific execution condition:

- **reachPoint**: The payload action fires instantaneously when the 3D coordinate of the specified waypoint is intercepted by the FMS.

- **multipleDistance**: The camera triggers at a continuous spatial interval (e.g., every 15 meters) across the entire index range.37

- **multipleTiming**: The camera triggers at a continuous temporal interval (e.g., every 2 seconds).37

- **betweenAdjacentPoints**: This trigger distributes an action evenly across a flight line, typically used for smooth, continuous gimbal pitching (gimbalEvenlyRotate) from one angle to another.37

By utilizing this index-based architecture, WPML supports dynamic triggering. A developer can define one <wpml:actionGroup> with a multipleDistance interval of 20 meters from Waypoint 1 to Waypoint 5, and immediately define a second <wpml:actionGroup> with a multipleDistance of 5 meters from Waypoint 5 to Waypoint 10. This allows the FMS to alter the spatial density of the captured data dynamically along the exact same continuous flight path without halting the vehicle.36

## senseFly (AgEagle) eMotion Flight Planning Architecture

The senseFly eBee series, now operating under AgEagle, utilizes a highly refined proprietary flight management software known as eMotion (currently deployed as eMotion 3).40 The eMotion ecosystem is specifically designed to eliminate the complexities of manual waypoint manipulation. The FMS outputs .plan and .mis files structured as proprietary XML documents that heavily abstract the photogrammetric process.

### eMotion.plan and XML Mission Structure

Unlike QGroundControl or DJI systems—which ultimately allow the user to manually drop, manipulate, and encode individual spatial waypoints—eMotion enforces a strict "mission block" paradigm.41 The proprietary XML file encodes the geospatial boundary polygon of the Area of Interest (AOI), which can be drawn natively or imported via KML. Accompanying this polygon, the user inputs the target Ground Sampling Distance (GSD) and the lateral/longitudinal overlap percentages (e.g., 75% frontal, 60% side overlap).42

Upon initiation, the eMotion software acts as a specialized compiler. It calculates the necessary Absolute Flight Altitude required to achieve the requested GSD based on the active sensor's known physical constraints—specifically the focal length and pixel pitch of payloads like the S.O.D.A. 3D or Aeria X camera.44 eMotion automatically generates the physical flight lines, the turn-around vectors required to clear the polygon boundaries, and the entry/exit points, writing these elements into the XML structure as sequential geocoordinates.44

### Altitude Handling: ATO, AMSL, and AED

eMotion utilizes three highly specific altitude reference frames to manage operational safety and photogrammetric consistency 46:

- **ATO (Above Take-Off):** Altitude measured relative to the physical launch location. This reference is mandatory for defining the Start and Home waypoints. Using ATO guarantees that multiple drones operating in a swarm have synchronized altitude separation layers during launch and recovery, mitigating collision risks over the Home waypoint.47

- **AMSL (Above Mean Sea Level):** Absolute altitude referencing a global geoid, used for transit and deconfliction with manned aviation.

- **AED (Above Elevation Data):** senseFly's proprietary implementation of terrain following. The eMotion XML mission file incorporates a sampled, low-resolution elevation profile. The FMS calculates an explicit AMSL altitude for every generated waypoint along the flight lines to maintain a constant AED. This ensures a perfectly uniform GSD across undulating or mountainous terrain.47

### Autonomous Photogrammetric Overlap Logic

A unique aspect of the eMotion FMS is that it entirely abstracts camera triggering away from the user and the mission file. The .plan XML format does not encode explicit Take Photo commands at individual waypoints or define static spatial intervals. Instead, the XML directly defines the required volumetric spatial overlap.44

During flight, the eBee's proprietary autopilot utilizes a combination of its onboard airspeed sensor (pitot tube), Inertial Measurement Unit (IMU), and RTK/PPK GNSS receiver to continuously calculate true ground speed dynamically.48 The autopilot's internal logic engine autonomously calculates when to fire the camera shutter (via an I2C/Serial hardware trigger pulse) precisely when the required physical spatial displacement is achieved. By utilizing internal state estimation rather than a rigid, pre-compiled mission file command, the eBee guarantees overlapping fidelity regardless of erratic wind speeds or atmospheric pressure changes.

## Leica Geosystems: HxMap, CityMapper, and FlightPro

In the realm of crewed airborne surveying and high-end, heavy-lift UAS platforms, Leica Geosystems operates a strictly controlled FMS ecosystem relying on the FlightPro and MissionPro software suites.50 These enterprise-grade systems produce and consume proprietary .xml and .pmp (Project Management Plan) formats, which dictate highly complex, multi-sensor orchestration.50

### FlightPro XML and PMP Mission Formats

Leica's .pmp and .xml flight plans are engineered for massive-scale, wide-area mapping operations. A MissionPro XML file functions differently from a UAS waypoint file; it is effectively a relational database export. It contains extensive metadata on the flight line vectors, cross-track spacing, stereoscopic overlap margins, and specific sensor payload indices required for a given project.51

The XML structure integrates directly with Leica’s central computing hardware, such as the CC33 or CC3X Sensor Controller modules.53 Heavy-lift platforms like the Leica CityMapper-2 consist of an integrated NovAtel SPAN GNSS/IMU, a 150-megapixel nadir RGB/NIR frame camera (MFC150), four 150-megapixel oblique cameras pitched at 45 degrees, and a Hyperion2+ LiDAR unit.54 Consequently, the mission file must encode the triggering parameters, focal length ratios, and latency offsets for the entire electromechanical array simultaneously.54

### Multi-Sensor Synchronization and Hardware Control

The XML mission file encodes the flight lines relative to local DTMs or global SRTM/ASTER databases. The primary function of the file is to ensure the pilot (or UAS autopilot) maintains the strict navigation corridor required to satisfy the specific Field of View (FOV) constraints of the multi-camera pod.55

Camera triggering within the Leica format is fundamentally spatial, governed by the required GSD and target overlap specified during the planning phase. However, the mission file also dictates the hard physical constraints of the electro-mechanical system. For instance, the XML configuration commands the CC33 controller to trigger the mechanical Forward Motion Compensation (FMC) central shutters up to their physical maximum frame rate (e.g., 0.9 seconds per cycle) while simultaneously dictating the LiDAR pulse repetition frequency (up to 2 MHz) and the mechanical scan pattern (e.g., Palmer or oblique sweep).54

As the autopilot navigates the geometric lines defined in the .xml file, the CC33 controller autonomously executes the complex firing sequence based on real-time spatial navigation data. It logs the exact nanosecond pulse and exposure events to solid-state mass memory (MM30 drives), ensuring absolute synchronization for rigorous post-processing within Leica HxMap.53

## Data Interoperability and Open Interchange Formats

Given the deep fragmentation of proprietary formats—from DJI's nested WPML to senseFly's abstracted eMotion XML and Leica's heavy-iron PMP database files—the commercial surveying industry relies heavily on open geospatial interchange formats to move planning boundaries between disparate ecosystems.

### KML and KML Tiles

The Keyhole Markup Language (KML/KMZ) serves as the lowest common denominator across virtually all platforms. Systems like QGroundControl, eMotion, DJI Pilot 2, and FlightPro all universally accept KML files.45 However, KML is strictly limited to defining the outer geometric boundary polygon of a survey area; it lacks the schema to convey complex FMS logic, overlap requirements, or camera parameter metadata. The planning software must use the ingested KML footprint to independently compile its proprietary flight lines.

### GeoPackage and GeoJSON (The IronTrack Paradigm)

To resolve the interoperability bottlenecks inherent in converting KMLs into proprietary FMS instructions, emerging open-source flight management initiatives are pushing toward database-backed, open standards. The IronTrack project, built on a decoupled, memory-safe Rust compute engine, exemplifies this paradigm shift.58

IronTrack utilizes GeoPackage—an Open Geospatial Consortium (OGC) standard built on SQLite—as its primary data store and exchange format.58 The GeoPackage format addresses the inherent limitations of flat XML or JSON files by allowing the FMS to encapsulate the planned flight trajectories, the spatial boundary polygons, cached offline terrain DEMs (such as SRTM or 3DEP), and the resulting operational metadata within a single, lightweight relational database.58

A decoupled FMS engine can ingest a standard WGS84 polygon, dynamically project it into a localized Coordinate Reference System (CRS) like UTM to perform geometrically accurate photogrammetric footprint calculations, and output the exact geometric line strings back into the GeoPackage. This mechanism ensures immediate interoperability with enterprise GIS software architectures (like QGIS or ESRI ArcGIS), entirely bypassing the need to parse, translate, or reverse-engineer proprietary autopilot configurations.58

## Conclusion

The architecture of aerial survey mission files dictates both the operational capabilities and the physical limits of the FMS and autopilot. Compliance with rigorous digital mapping frameworks, such as the ASPRS Positional Accuracy Standards, requires FMS software to flawlessly translate geometric polygons into precise spatial instructions.

The transition from MAVLink 1.0 to 2.0 demonstrates how fundamental mathematical limitations—specifically the inability of 32-bit floating-point variables to maintain global centimeter precision—forced a protocol shift to 32-bit scaled integers (MISSION_ITEM_INT), a necessity to support RTK/PPK photogrammetry.

While open-source systems like ArduPilot push computation to the edge, allowing the autopilot to read mission files and calculate dynamic terrain-following profiles in real-time, enterprise systems like DJI offload this computational weight. DJI's WPML structure rigidly compiles absolute EGM96 altitudes and defines highly granular, dynamic camera triggering intervals via its <wpml:actionGroup> XML hierarchy. High-end platforms like Leica Geosystems utilize XML/PMP files not just for spatial navigation, but to synchronize intricate electromechanical sensor arrays with high-frequency GNSS/IMU data.

Ultimately, navigating the fragmented landscape of FMS software requires a nuanced understanding of how differing file formats encode altitude datums, spatial overlaps, and camera triggers. As the aerial surveying industry continues to advance toward interconnected Digital Twins and automated corridor mapping, the adoption of open, database-backed formats like GeoPackage, combined with robust, cross-platform protocols like MAVLink 2.0, will remain critical for achieving seamless interoperability across diverse airborne platforms.

#### Works cited

- sur 4501c/6502c • foundations of uas mapping - UF/IFAS School of Forest, Fisheries, and Geomatics Sciences, accessed March 23, 2026, [https://ffgs.ifas.ufl.edu/media/ffgsifasufledu/docs/pdf/all-courses/ffgs-all-courses-syllabi/ffgs-all-courses-syllabi/sur-courses/SUR-4501C---Foundations-of-UAS-Mapping.pdf](https://ffgs.ifas.ufl.edu/media/ffgsifasufledu/docs/pdf/all-courses/ffgs-all-courses-syllabi/ffgs-all-courses-syllabi/sur-courses/SUR-4501C---Foundations-of-UAS-Mapping.pdf)

- Workshops & Trainings - Geo Week, accessed March 23, 2026, [https://www.geo-week.com/workshops-trainings/](https://www.geo-week.com/workshops-trainings/)

- Best Practices for Acquisition and Processing of Oblique Imagery - Geo Week, accessed March 23, 2026, [https://www.geo-week.com/session/best-practices-for-acquisition-and-processing-of-oblique-imagery/](https://www.geo-week.com/session/best-practices-for-acquisition-and-processing-of-oblique-imagery/)

- Framework for the Verification of Geometric Digital Twins: Application in a University Environment - MDPI, accessed March 23, 2026, [https://www.mdpi.com/2075-5309/15/21/3854](https://www.mdpi.com/2075-5309/15/21/3854)

- Point Cloud‒Based UAV Mapping within Spatial Data Infrastructures - Oscar Publishing Services, accessed March 23, 2026, [https://theusajournals.com/index.php/ajahi/article/download/8990/8328/13171](https://theusajournals.com/index.php/ajahi/article/download/8990/8328/13171)

- NATIONAL GUIDELINES FOR DIGITAL CAMERA SYSTEMS CERTIFICATION - Semantic Scholar, accessed March 23, 2026, [https://pdfs.semanticscholar.org/2fd0/fe5733071a5b66077296d8b7c362a2ceb579.pdf](https://pdfs.semanticscholar.org/2fd0/fe5733071a5b66077296d8b7c362a2ceb579.pdf)

- UAV Positioning Using GNSS: A Review of the Current Status - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/publication/400200027_UAV_Positioning_Using_GNSS_A_Review_of_the_Current_Status](https://www.researchgate.net/publication/400200027_UAV_Positioning_Using_GNSS_A_Review_of_the_Current_Status)

- Waylines.wpml - Cloud API - DJI Developer, accessed March 23, 2026, [https://developer.dji.com/doc/cloud-api-tutorial/en/api-reference/dji-wpml/waylines-wpml.html](https://developer.dji.com/doc/cloud-api-tutorial/en/api-reference/dji-wpml/waylines-wpml.html)

- Template.kml - DJI Developer, accessed March 23, 2026, [https://developer.dji.com/doc/cloud-api-tutorial/en/api-reference/dji-wpml/template-kml.html](https://developer.dji.com/doc/cloud-api-tutorial/en/api-reference/dji-wpml/template-kml.html)

- Terrain Protocol - MAVLink Guide, accessed March 23, 2026, [https://mavlink.io/en/services/terrain.html](https://mavlink.io/en/services/terrain.html)

- Terrain Following — Plane documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/plane/docs/common-terrain-following.html](https://ardupilot.org/plane/docs/common-terrain-following.html)

- Terrain Following (in Auto, Guided, etc) — Copter documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/copter/docs/terrain-following.html](https://ardupilot.org/copter/docs/terrain-following.html)

- Plan File Format - QGC Guide, accessed March 23, 2026, [https://docs.qgroundcontrol.com/master/en/qgc-dev-guide/file_formats/plan.html](https://docs.qgroundcontrol.com/master/en/qgc-dev-guide/file_formats/plan.html)

- Mission: JSON file format · Issue #3800 · mavlink/qgroundcontrol - GitHub, accessed March 23, 2026, [https://github.com/mavlink/qgroundcontrol/issues/3800](https://github.com/mavlink/qgroundcontrol/issues/3800)

- Micro Air Vehicle Link (MAVLink) in a Nutshell: A Survey - arXiv, accessed March 23, 2026, [https://arxiv.org/pdf/1906.10641](https://arxiv.org/pdf/1906.10641)

- Protocol Overview - MAVLink Guide, accessed March 23, 2026, [https://mavlink.io/en/about/overview.html](https://mavlink.io/en/about/overview.html)

- Review on MAV Link for Unmanned Air Vehicle to Ground Control Station Communication - Think India Journal, accessed March 23, 2026, [https://thinkindiaquarterly.org/index.php/think-india/article/download/10158/5882/](https://thinkindiaquarterly.org/index.php/think-india/article/download/10158/5882/)

- MAVLINK Common Message Set - Hamish Willee, accessed March 23, 2026, [https://hamishwillee.gitbooks.io/ham_mavdevguide/kr/messages/common.html](https://hamishwillee.gitbooks.io/ham_mavdevguide/kr/messages/common.html)

- A Novel Cipher for Enhancing MAVLink Security: Design, Security Analysis, and Performance Evaluation Using a Drone Testbed - IEEE Xplore, accessed March 23, 2026, [https://ieeexplore.ieee.org/iel8/8782661/10829557/11202964.pdf](https://ieeexplore.ieee.org/iel8/8782661/10829557/11202964.pdf)

- Mission Protocol - MAVLink Guide, accessed March 23, 2026, [https://mavlink.io/en/services/mission.html](https://mavlink.io/en/services/mission.html)

- Mission Protocol - MAVLink Guide, accessed March 23, 2026, [https://mavlink.io/zh/services/mission.html](https://mavlink.io/zh/services/mission.html)

- MAVLINK Common Message Set (common.xml), accessed March 23, 2026, [https://mavlink.io/en/messages/common.html](https://mavlink.io/en/messages/common.html)

- ardupilot-releases/libraries/GCS_MAVLink/include/mavlink/v1.0/common/mavlink_msg_mission_item_int.h at master · dronekit/ardupilot-releases · GitHub, accessed March 23, 2026, [https://github.com/dronekit/ardupilot-releases/blob/master/libraries/GCS_MAVLink/include/mavlink/v1.0/common/mavlink_msg_mission_item_int.h](https://github.com/dronekit/ardupilot-releases/blob/master/libraries/GCS_MAVLink/include/mavlink/v1.0/common/mavlink_msg_mission_item_int.h)

- Mission Commands — Copter documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/copter/docs/common-mavlink-mission-command-messages-mav_cmd.html](https://ardupilot.org/copter/docs/common-mavlink-mission-command-messages-mav_cmd.html)

- Mission Commands — Rover documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/rover/docs/common-mavlink-mission-command-messages-mav_cmd.html](https://ardupilot.org/rover/docs/common-mavlink-mission-command-messages-mav_cmd.html)

- Mission Commands — Plane documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/plane/docs/common-mavlink-mission-command-messages-mav_cmd.html](https://ardupilot.org/plane/docs/common-mavlink-mission-command-messages-mav_cmd.html)

- Messages V2 - 7.7 - Parrot, accessed March 23, 2026, [https://developer.parrot.com/docs/mavlink-flightplan/messages_v2.html](https://developer.parrot.com/docs/mavlink-flightplan/messages_v2.html)

- ardupilotmega/ardupilotmega.h · master · gitadmin-cvr / C Library V2 - GitLab, accessed March 23, 2026, [https://gitlab.espol.edu.ec/gitadmin-cvr/c_library_v2/-/blob/master/ardupilotmega/ardupilotmega.h](https://gitlab.espol.edu.ec/gitadmin-cvr/c_library_v2/-/blob/master/ardupilotmega/ardupilotmega.h)

- Camera Control in Auto Missions — Copter documentation - ArduPilot, accessed March 23, 2026, [https://ardupilot.org/copter/docs/common-camera-control-and-auto-missions-in-mission-planner.html](https://ardupilot.org/copter/docs/common-camera-control-and-auto-missions-in-mission-planner.html)

- mavlink - Go Packages, accessed March 23, 2026, [https://pkg.go.dev/github.com/liamstask/go-mavlink/mavlink](https://pkg.go.dev/github.com/liamstask/go-mavlink/mavlink)

- MAVLINK Common Message Set - Hamish Willee, accessed March 23, 2026, [https://hamishwillee.gitbooks.io/ham_mavdevguide/en/messages/common.html](https://hamishwillee.gitbooks.io/ham_mavdevguide/en/messages/common.html)

- common/common.h · mavlink2 · gitadmin-cvr / C Library V2 - GitLab, accessed March 23, 2026, [https://gitlab.espol.edu.ec/gitadmin-cvr/c_library_v2/-/blob/mavlink2/common/common.h?ref_type=heads](https://gitlab.espol.edu.ec/gitadmin-cvr/c_library_v2/-/blob/mavlink2/common/common.h?ref_type=heads)

- terrain following in ardupilot/master - Google Groups, accessed March 23, 2026, [https://groups.google.com/g/drones-discuss/c/8LsinXD0vqw](https://groups.google.com/g/drones-discuss/c/8LsinXD0vqw)

- Wayline Management Sample - DJI Developer, accessed March 23, 2026, [https://developer.dji.com/doc/mobile-sdk-tutorial/en/tutorials/waypoint.html](https://developer.dji.com/doc/mobile-sdk-tutorial/en/tutorials/waypoint.html)

- Mission Planning and Management Tutorial - Autel Developer Technologies, accessed March 23, 2026, [https://developer.autelrobotics.com/doc/v2.5/mobile_sdk/en/50/7](https://developer.autelrobotics.com/doc/v2.5/mobile_sdk/en/50/7)

- 20.template-kml.md - GitHub, accessed March 23, 2026, [https://github.com/dji-sdk/Cloud-API-Doc/blob/master/docs/en/60.api-reference/00.dji-wpml/20.template-kml.md](https://github.com/dji-sdk/Cloud-API-Doc/blob/master/docs/en/60.api-reference/00.dji-wpml/20.template-kml.md)

- Cloud-API-Doc/docs/en/60.api-reference/00.dji-wpml/40.common-element.md at master · dji ... - GitHub, accessed March 23, 2026, [https://github.com/dji-sdk/Cloud-API-Doc/blob/master/docs/en/60.api-reference/00.dji-wpml/40.common-element.md](https://github.com/dji-sdk/Cloud-API-Doc/blob/master/docs/en/60.api-reference/00.dji-wpml/40.common-element.md)

- [OPEN BETA] Litchi Hub - Page 10 - Beta Feedback - Litchi Forum, accessed March 23, 2026, [https://forum.flylitchi.com/t/open-beta-litchi-hub/23994?page=10](https://forum.flylitchi.com/t/open-beta-litchi-hub/23994?page=10)

- 4 - CodaLab, accessed March 23, 2026, [https://worksheets.codalab.org/rest/bundles/0xd74f36104e7244e8ad99022123e78884/contents/blob/frequent-classes](https://worksheets.codalab.org/rest/bundles/0xd74f36104e7244e8ad99022123e78884/contents/blob/frequent-classes)

- Emotion 3 User Manual | PDF | Damages | Sea Level - Scribd, accessed March 23, 2026, [https://www.scribd.com/document/576911880/eMotion-3-User-Manual](https://www.scribd.com/document/576911880/eMotion-3-User-Manual)

- eMotion 3 - Flight Planning Software | Sensefly - OPTRON, accessed March 23, 2026, [https://optron.com/sensefly/portfolio/emotion-3/](https://optron.com/sensefly/portfolio/emotion-3/)

- Planning a mission using eMotion software (senseFly SA, Lausanne,... - ResearchGate, accessed March 23, 2026, [https://www.researchgate.net/figure/Planning-a-mission-using-eMotion-software-senseFly-SA-Lausanne-Switzerland-adjusting_fig1_322065359](https://www.researchgate.net/figure/Planning-a-mission-using-eMotion-software-senseFly-SA-Lausanne-Switzerland-adjusting_fig1_322065359)

- 3. Flight Plan Creation - Drone Harmony, accessed March 23, 2026, [https://www.droneharmony.com/post/3-flight-plan-creation](https://www.droneharmony.com/post/3-flight-plan-creation)

- The professional mapping drone - OPTRON, accessed March 23, 2026, [http://optron.com/sensefly/wp-content/uploads/2018/02/ebee_en.pdf](http://optron.com/sensefly/wp-content/uploads/2018/02/ebee_en.pdf)

- senseFly drones for professionals, accessed March 23, 2026, [https://srv.jgc.gr/Pdf_files/sensefly-drones-for-professionals.pdf](https://srv.jgc.gr/Pdf_files/sensefly-drones-for-professionals.pdf)

- Leica MissionPro Flight Planning Software, accessed March 23, 2026, [https://leica-geosystems.com/es-es/products/airborne-systems/software/leica-missionpro](https://leica-geosystems.com/es-es/products/airborne-systems/software/leica-missionpro)

- senseFly Academy — Coordinates & Altitude References in eMotion 3 - YouTube, accessed March 23, 2026, [https://www.youtube.com/watch?v=5hidfrEAM_E](https://www.youtube.com/watch?v=5hidfrEAM_E)

- User Manual: eBee, accessed March 23, 2026, [https://sefd5f829031e8300.jimcontent.com/download/version/1613830287/module/13971464392/name/SenseFly%20eBee%20Drone%20User%20Manual.pdf](https://sefd5f829031e8300.jimcontent.com/download/version/1613830287/module/13971464392/name/SenseFly%20eBee%20Drone%20User%20Manual.pdf)

- eBee - senseFly - PDF Catalogs | Technical Documentation | Brochure, accessed March 23, 2026, [https://pdf.agriexpo.online/pdf/sensefly/ebee/170212-5751.html](https://pdf.agriexpo.online/pdf/sensefly/ebee/170212-5751.html)

- Leica FlightPro Flight Management & Control System, accessed March 23, 2026, [https://leica-geosystems.com/products/airborne-systems/software/leica-flightpro](https://leica-geosystems.com/products/airborne-systems/software/leica-flightpro)

- Product Description Leica MissionPro, Version 11, accessed March 23, 2026, [https://www.geospace.co.za/wp-content/uploads/2017/10/Leica_MissionPro_ProductDesc_en.pdf](https://www.geospace.co.za/wp-content/uploads/2017/10/Leica_MissionPro_ProductDesc_en.pdf)

- LeicaMissionPro 2 | PDF | Copyright | Databases - Scribd, accessed March 23, 2026, [https://www.scribd.com/document/622129725/LeicaMissionPro-2](https://www.scribd.com/document/622129725/LeicaMissionPro-2)

- Leica CityMapper More information, smarter decisions, accessed March 23, 2026, [https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica_citymapper_hybrid_sensor_ds.ashx?la=en](https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica_citymapper_hybrid_sensor_ds.ashx?la=en)

- Leica CityMapper-2 More information, smarter decisions, accessed March 23, 2026, [https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica%20citymapper-2%20ds.ashx](https://leica-geosystems.com/-/media/files/leicageosystems/products/datasheets/leica%20citymapper-2%20ds.ashx)

- Leica MissionPro Flight Planning Software, accessed March 23, 2026, [https://leica-geosystems.com/de-at/products/airborne-systems/software/leica-missionpro](https://leica-geosystems.com/de-at/products/airborne-systems/software/leica-missionpro)

- Leica MissionPro BRO | PDF | Lidar | Databases - Scribd, accessed March 23, 2026, [https://www.scribd.com/document/637793461/Leica-MissionPro-BRO](https://www.scribd.com/document/637793461/Leica-MissionPro-BRO)

- LiDAR Data Processing Manual-V1 | PDF - Scribd, accessed March 23, 2026, [https://www.scribd.com/document/920882291/LiDAR-Data-Processing-Manual-V1](https://www.scribd.com/document/920882291/LiDAR-Data-Processing-Manual-V1)

- Irontrack