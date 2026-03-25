# Engineering Analysis of Flight Time and Energy Consumption Estimation Models for Photogrammetric UAV Missions

## 1. Introduction to Unmanned Aerial Vehicle Survey Modeling

The proliferation of Unmanned Aerial Vehicles (UAVs) has fundamentally transformed the acquisition of high-resolution geospatial data, establishing aerial photogrammetry as a critical tool for topographic surveying, precision agriculture, and infrastructure monitoring. A profound operational constraint in deploying both fixed-wing and multirotor UAV platforms is the stringent limitation on onboard energy storage. This limitation dictates the maximum achievable flight time and, consequently, the spatial extent of the survey area that can be reliably captured within a single deployment.1 Precise estimation of mission flight time and energy consumption is paramount for safe and efficient operations. Underestimating energy requirements can result in catastrophic vehicle loss or incomplete data acquisition, while overestimating leads to suboptimal resource utilization, unnecessary battery cycling, and excessive operational downtime.3

Photogrammetric survey missions are kinematically distinct from generic point-to-point navigation due to their rigorous geometric constraints. These missions typically utilize boustrophedon (lawnmower) flight paths to ensure a uniform Ground Sampling Distance (GSD) and specific frontal and lateral image overlaps, typically requiring 80% frontal and 60% lateral overlap to satisfy the feature-matching requirements of Structure-from-Motion (SfM) algorithms.5 These optical parameters inextricably link the vehicle's aerodynamic performance with the physics of the camera sensor. Generating a three-dimensional path that fulfills these photogrammetric requirements while minimizing energy consumption requires the complex integration of discrete kinematic modeling, continuous dynamic control algorithms, and platform-specific power consumption profiles.7

This analysis provides an exhaustive engineering evaluation of flight time and energy consumption estimation models tailored for photogrammetric UAV operations. It systematically dissects the standard kinematic equations governing straight-line transits, acceleration and deceleration phases, and coordinated turns within local coordinate frames. Furthermore, it explores the fundamental divergence between fixed-wing aerodynamic endurance models, derived from the classical Breguet range equation, and multirotor power dynamics, characterized by complex, non-linear U-shaped power curves. The analysis evaluates the empirical performance of industry-standard platforms, assesses the algorithmic simplifications embedded within commercial mission planning software, and outlines the mathematical optimization of non-productive turnaround patterns between parallel flight lines.

## 2. Standard Kinematic Models for Mission Flight Time Estimation

The total flight time of a UAV executing a waypoint-based photogrammetric mission cannot be accurately predicted by simply dividing the total path length by a nominal cruise speed. Instead, it is the cumulative sum of the durations of distinct kinematic phases: straight-line transit along the active flight lines, longitudinal acceleration and deceleration during the ingress and egress of these lines, and the execution of coordinated turns during non-productive turnaround maneuvers.9 Mathematical models of these phases must account for the physical constraints of the airframe and the prevailing environmental conditions.

### 2.1 Straight-Line Transit and Constant Velocity Phases

The vast majority of a photogrammetric mission is spent in steady-state, straight-line flight, during which the optical sensor actively acquires data. In a simplified, zero-wind environment, the transit time for a flight line segment of length L at a constant ground speed V is trivially defined as t = L / V.11 However, the introduction of atmospheric wind fields necessitates a more complex kinematic representation. In a local Earth-fixed East-North-Up (ENU) reference frame, the UAV's state matrix incorporates its position coordinates (x, y) and its attitude information, specifically the pitch angle θ and the yaw angle ψ.12

The fundamental kinematic equations governing the fixed-wing UAV's motion in the horizontal plane, subject to a steady wind field, are defined by the vector addition of the vehicle's airspeed and the wind speed. The rate of change of the vehicle's position is given by:

```
ẋ = V × cos(ψ) + W_x
ẏ = V × sin(ψ) + W_y
ψ̇ = ω
```

In these equations, ẋ and ẏ represent the velocity components along the North-South and East-West axes, respectively, V is the true airspeed of the aerial vehicle, ψ is the heading measured clockwise from the North axis, ω is the turn rate, and W_x and W_y are the orthogonal components of the prevailing wind.8

For a required ground track angle necessary to follow a predefined photogrammetric flight line, the presence of a crosswind necessitates that the UAV adopt a crab angle to maintain its trajectory over the target geometry. The requisite heading and the resulting ground speed magnitude  must be continuously recalculated to ensure that the image overlap timing remains consistent.14 If the flight planning software commands a constant airspeed , the ground speed will fluctuate significantly depending on whether the aircraft is flying a headwind or tailwind leg, thereby altering the temporal duration of each parallel sweep. Conversely, if the flight controller commands a constant ground speed  to maintain precise spatial overlap for the camera triggers, the UAV must dynamically adjust its thrust and airspeed, which directly and non-linearly impacts the power draw from the battery.15

### 2.2 Acceleration and Deceleration Dynamics

Traditional flight planners often model trajectories assuming instantaneous velocity changes at waypoints. This mathematical simplification inherently underestimates the total mission time, as real-world UAVs are bound by rigid physical limitations on longitudinal acceleration and deceleration, dictated by maximum available thrust, mass inertia, and aerodynamic drag.1

During the entry and exit of a flight line, the UAV transitions between the maneuvering speed utilized during the turnaround and the designated cruise speed required for data acquisition. The kinematics of motion under a constant acceleration limit a_max can be modeled using standard discrete-time motion equations, where the final velocity v_f and distance d over time t are derived as v_f = v_i + a×t and d = v_i×t + ½×a×t².17

For multirotor UAVs, longitudinal acceleration is achieved by pitching the airframe to redirect a component of the vertical thrust vector horizontally. This maneuver temporarily increases the total thrust required to maintain altitude, thereby inducing severe power spikes.18 Field data and comprehensive energy modeling demonstrate that the maximum power demands during an accelerated climb or forward acceleration phase can reach up to 220% of the nominal cruise power.19 Ignoring these acceleration phases in mission planning algorithms leads to compounding errors in energy estimation. Because the relationship between thrust and electrical power draw is highly non-linear, brief periods of high acceleration disproportionately deplete battery reserves, making acceleration-aware path planning critical for time-sensitive or energy-constrained applications.16 Faster trajectories might consume significantly more energy during acceleration and deceleration phases, but if the total flight time is sufficiently reduced, the integral of energy consumption over the entire mission may paradoxically be lower than that of a slower, constant-velocity trajectory.1

### 2.3 Coordinated Turns and Kinematic Constraints

The transition between parallel flight lines requires the UAV to execute a heading reversal. For fixed-wing aircraft, this is achieved through a coordinated turn, where the aircraft rolls to bank angle , generating a horizontal component of lift that acts as the centripetal force required to turn the vehicle without slipping or skidding.20

The fundamental kinematic equations governing a steady, coordinated turn in a zero-wind environment dictate the turn radius R and the turn rate ω as functions of the true airspeed V and the bank angle φ:

```
R = V² / (g × tan(φ))
ω = g × tan(φ) / V
```

where g represents the acceleration due to gravity.21 To avoid aerodynamic stall conditions, altitude loss, and structural overstress (excessive load factors), the maximum allowable bank angle φ_max is strictly constrained by the flight controller. This constraint inherently defines the Minimum Turn Radius (MTR) of the aircraft at a given cruise speed: R_min = V² / (g × tan(φ_max)).1 Time spent executing these turns is considered non-productive from a photogrammetric standpoint, as the camera is typically inactive, or the acquired imagery is highly oblique and unusable for orthomosaic generation due to the steep bank angle. The accurate integration of the time spent executing these sweeping arcs represents a critical component of the total mission time mathematical model, as the path length of a curve significantly exceeds the linear distance between the endpoints of two parallel survey lines.

## 3. Fixed-Wing Endurance and Performance Models

Fixed-wing UAVs rely on the aerodynamic efficiency of their lifting surfaces to support the vehicle's weight, allowing the propulsion system to primarily focus on overcoming profile and parasitic drag. This fundamental decoupling of lift and thrust generation enables significantly longer endurance and higher cruise speeds compared to multirotors, establishing fixed-wing platforms as the optimal choice for large-scale, long-baseline photogrammetric surveys.12

### 3.1 The Breguet Range Equation and Electric Adaptations

The foundational mathematical model for fixed-wing performance is the Breguet range equation. For traditional internal combustion engine (ICE) or turbojet aircraft, the classical Breguet equation models maximum range R as a function of the continuous reduction in mass as hydrocarbon fuel is consumed over the course of the flight 27:

```
R = (V / SFC) × (L/D) × ln(W_initial / W_final)
```

where V is the true airspeed, SFC is the thrust-specific fuel consumption, L/D is the aerodynamic lift-to-drag ratio, W_initial is the initial gross weight at takeoff, and W_final is the final weight after fuel burn.28

However, the vast majority of commercial survey UAVs operate on battery-electric propulsion systems. Because the mass of a lithium-polymer (LiPo) or lithium-ion (Li-ion) battery remains constant as it discharges electrical energy, the classical mass-reduction Breguet equation is rendered obsolete and must be reformulated for electric flight.30

For battery-powered UAVs, the modified Breguet range equation equates the total range to the total usable energy stored onboard, assuming steady-level flight conditions. The electric range R_e is formulated as:

```
R_e = (η_total × E_battery × L/D) / (m × g)
```

where η_total represents the aggregate efficiency of the entire powertrain (including the electronic speed controller, the electric motor, and the propeller),  is the usable energy capacity of the battery in Joules (often derived from specific energy density E_s), m is the constant mass of the UAV including its payload, and g is the acceleration due to gravity.32

This formulation yields critical operational insights. To maximize the absolute range of the vehicle, the fixed-wing UAV must fly at the specific velocity that yields the maximum aerodynamic lift-to-drag ratio, denoted as (L/D)_max.29 Similarly, to maximize endurance (the total time aloft regardless of distance covered), the aircraft must minimize the power required to sustain level flight. This occurs at the velocity corresponding to the maximum value of C_L^(3/2) / C_D, which is strictly lower than the maximum range speed.29

### 3.2 Cruise Speed Selection and GSD-Blur Constraints

In the context of photogrammetric mapping, the purely aerodynamic optimization of cruise speed directly conflicts with the physical limitations of the optical payload. The primary objective of an aerial survey is the acquisition of perfectly sharp, nadir imagery. As the drone traverses the flight line, the forward motion of the camera over the terrain while the mechanical or electronic shutter is open inherently causes motion blur.

Motion blur is quantified mathematically as the physical distance traveled by the sensor footprint over the ground during the exposure time:

For engineering-grade mapping and 3D modeling, industry photogrammetric standards dictate that motion blur must not exceed 1.0 to 2.0 times the Ground Sampling Distance (GSD), with values under 0.5 pixels highly preferred for precise crack detection or structural analysis.34 Consequently, the maximum allowable ground speed  of the UAV is strictly constrained by the camera's fastest capable shutter speed and the desired GSD 36:

If the environmental lighting conditions are poor (e.g., heavy overcast), requiring a slower shutter speed to prevent the camera from utilizing a high ISO—which introduces digital noise and degrades the point cloud matching algorithms—the maximum allowable flight speed must be proportionally reduced.35

This dynamic creates a critical aero-optical compromise. If the optically constrained speed  is significantly lower than the aircraft's optimal endurance speed (minimum power speed) or optimal range speed (minimum drag speed), the UAV is forced to fly at a high angle of attack to generate sufficient lift at low speeds. Operating at a high angle of attack drastically increases lift-induced drag, severely degrading the  ratio and causing a massive penalty to the total mission endurance.37 Therefore, selecting the cruise speed is a complex, multidisciplinary optimization problem that must balance the Breguet aerodynamic efficiency curve against the strict GSD-blur constraint threshold.

### 3.3 Wind Vector Impacts and Fuel Burn Penalties

The presence of a steady atmospheric wind field alters both the kinematics and the energy dynamics of the survey mission. When flight lines are oriented parallel to the prevailing wind vector, yielding pure headwind and tailwind legs, the aircraft maintains a constant heading but experiences dramatic shifts in its ground speed.14

On a tailwind leg, the ground speed increases significantly. To avoid violating the maximum motion blur constraint, the aircraft may need to reduce its airspeed, though it is fundamentally limited by its stall speed. Conversely, on a headwind leg, the ground speed decreases. While a lower ground speed eliminates any motion blur concerns, the temporal duration required to complete the flight line increases significantly.14 Because the UAV draws a continuous, constant baseline power for its avionics, payload sensors, and control surface actuation regardless of its speed over the ground, spending more time in the air to cover the exact same spatial distance results in a higher total energy consumption per unit of area mapped.38 Furthermore, pushing against a strong headwind to maintain a minimum acceptable ground speed for operational efficiency requires the propulsion system to operate at higher RPMs, increasing the aerodynamic power draw cubically relative to the airspeed.40

Orienting the photogrammetric flight lines perpendicular to the wind minimizes the temporal discrepancy between opposing legs, but introduces a severe crosswind component. To counteract lateral drift and remain on the prescribed flight line, the UAV must maintain a continuous crab angle.21 This non-zero sideslip angle exposes the fuselage flank to the freestream airflow, massively increasing parasitic drag and subsequently increasing the total mission energy consumption.14 Consequently, advanced decomposition-based mission planners optimize flight line orientation to balance the aerodynamic resistance penalties against the frequency of turnaround maneuvers.38

## 4. Multirotor Energy and Power Consumption Models

Unlike fixed-wing aircraft, multirotor UAVs lack aerodynamic lifting surfaces and must generate both lift and directional thrust exclusively through the continuous rotation of their propellers. The energy dynamics of multirotors are highly non-linear and profoundly sensitive to variations in payload mass and horizontal flight velocity, requiring advanced physical models based on momentum theory to accurately predict mission endurance.18

### 4.1 Aerodynamic Power Dynamics: Hover vs. Forward Flight

The total power consumption of a multirotor UAV is the summation of three primary aerodynamic power components, alongside a constant baseline draw for avionics and payloads: induced power (P_induced), profile power (P_profile), and parasitic power (P_parasitic).40

The induced power represents the energy required to accelerate a mass of air downward through the rotor disk to generate vertical thrust. According to classical momentum theory, the induced power is the product of the thrust  and the induced velocity v_i of the air at the rotor disk (P_induced = T × v_i).19 In a stationary hover, induced power accounts for the vast majority of the total energy consumption. The profile power is the energy required to overcome the aerodynamic drag of the rotating propeller blades cutting through the air, which scales with the cube of the blade tip velocity.40 Finally, the parasite power is the energy required to overcome the aerodynamic drag of the entire UAV fuselage moving horizontally through the air. Parasite power is strictly zero in a perfect hover and scales with the cube of the forward velocity (P_parasitic ∝ V³).40

### 4.2 The U-Shaped Power Curve and Optimal Forward Velocity

A critical engineering insight, derived from extensive empirical testing and Blade Element Momentum Theory (BEMT) models developed by research institutions such as ETH Zurich and DLR, is the complex relationship between forward flight speed and total power consumption.37

As a multirotor transitions from a stationary hover into forward flight, the relative motion of the freestream air entering the rotor disk increases horizontally. This horizontal airflow reduces the induced velocity  required to produce a given amount of vertical thrust, a highly beneficial aerodynamic phenomenon known as translational lift.18 As a direct result, the massive induced power requirement actually decreases as the forward speed increases from zero.

Simultaneously, however, the parasite drag of the airframe increases quadratically with speed, requiring the multirotor to pitch forward to generate a horizontal thrust component to overcome this drag, which in turn increases the total power demand.40 The superposition of the decreasing induced power curve and the rapidly increasing parasite power curve results in a distinct, mathematically predictable **U-shaped power curve**.43

This U-shaped curve demonstrates unequivocally that hovering is *not* the most energy-efficient state for a multirotor. There exists an optimal forward velocity (V_opt) at the nadir of the U-curve where the absolute total power consumption is minimized. Furthermore, to maximize the total range of the multirotor, the metric of greatest interest is the Energy Per Meter (EPM), defined as EPM = P_total / V. Minimizing the EPM yields the maximum range speed, which typically occurs at a higher velocity than the minimum power speed, as flying faster covers more ground before the baseline avionic power drains the battery.18

### 4.3 Payload Mass Effects on Energy Efficiency

In professional photogrammetry, swapping optical payloads (e.g., transitioning from a lightweight 24MP RGB camera to a heavy, multi-sensor LiDAR system) fundamentally alters the gross mass of the UAV. Because a multirotor must constantly generate thrust equal to its weight to maintain altitude (T = m×g), payload mass directly and severely dictates the baseline power draw.18

Recent analytical derivations utilizing the BEMT models have established a profound relationship: the optimum Energy Per Meter traveled per unit mass (EPM/m) remains constant under varying vehicle masses, provided the drone flies exactly at the energy-optimal velocity corresponding to that specific mass.18 Crucially, the mathematical models prove that as the payload mass  increases, the optimal forward flight velocity must also increase, scaling proportionally to the square root of the mass (V_opt ∝ √m).18 Furthermore, the optimal pitch angle required to achieve this state of maximum efficiency remains constant regardless of the total vehicle mass.18

However, this theoretical aerodynamic efficiency is frequently unattainable in practical survey missions due to the aforementioned GSD-motion blur constraints.34 If a heavy payload dictates that the aerodynamically optimal velocity is 15 m/s, but the camera's shutter speed limits the mission to 8 m/s to prevent image blurring, the multirotor is forced to operate on the steep, inefficient low-speed side of the U-shaped curve. This operational conflict results in a dramatic reduction in achievable flight time and total area coverage.45

### 4.4 Empirical Models: DJI Matrice 300/350 RTK vs. senseFly eBee X

To ground these theoretical models in operational reality, it is instructive to compare the empirical performance of industry-standard platforms utilizing structured data.

The DJI Matrice series exemplifies the heavy-lift enterprise multirotor paradigm. Both the M300 RTK and M350 RTK feature a maximum theoretical flight time of 55 minutes, but this figure is only achievable in an unloaded state.46 The airframe supports a maximum takeoff weight of 9.0 kg (M300) and 9.2 kg (M350), with a maximum payload capacity of 2.7 kg.46 Empirical power consumption regression models generated from extensive flight logs reveal that integrating a full surveying payload—such as the Zenmuse L1 LiDAR or the P1 full-frame photogrammetry camera—drops the functional endurance to approximately 31 to 43 minutes.50 The M350 introduces the upgraded TB65 battery system, improving lifecycle degradation (400 cycles compared to the TB60's 200 cycles) and enhancing the voltage discharge stability under heavy current loads, though the absolute endurance limits remain rigidly tied to the physical U-shaped aerodynamic power curve.49 Both platforms can tolerate wind speeds up to 15 m/s (M300) and 12 m/s (M350), but flying near these limits requires extreme pitch angles, increasing the body drag profile and precipitating rapid battery depletion.47

Representing the fixed-wing paradigm, the senseFly eBee X provides a stark contrast in aerodynamic efficiency. With a maximum takeoff weight of just 1.6 to 2.2 kg depending on the integrated payload (e.g., S.O.D.A or Aeria X), it requires a fraction of the raw mechanical power to remain aloft.53 Utilizing the Breguet range optimization principles, the eBee X achieves a baseline endurance of 59 minutes, which extends to a remarkable 90 minutes when utilizing a high-voltage LiHV Endurance battery.54 Because lift is generated passively by the wings rather than by active thrust, the eBee X can maintain a high cruise speed of 11 to 30 m/s (40-110 km/h) 54 and cover up to 500 hectares (1,235 acres) in a single deployment.54 The endurance of the fixed-wing platform is more sensitive to atmospheric turbulence and frequent altitude changes, but the fundamental aerodynamic decoupling of lift and thrust makes its per-hectare energy expenditure an order of magnitude lower than that of the heavy-lift multirotors.26

| **Platform Parameter** | **DJI Matrice 300 RTK (Multirotor)** | **DJI Matrice 350 RTK (Multirotor)** | **senseFly eBee X (Fixed-Wing)** |
| --- | --- | --- | --- |
| **Max Takeoff Weight** | 9.0 kg 49 | 9.2 kg 49 | 1.6 - 2.2 kg 53 |
| **Max Payload Capacity** | 2.7 kg 49 | 2.7 kg 49 | Integrated Sensors (S.O.D.A, Aeria X) 55 |
| **Max Flight Time (No Payload)** | 55 minutes 49 | 55 minutes 49 | 90 minutes (with Endurance Battery) 54 |
| **Functional Survey Endurance** | ~31 - 43 mins (with Zenmuse P1) 50 | ~31 - 43 mins (with Zenmuse P1) 51 | ~90 mins (with Aeria X) 55 |
| **Max Wind Resistance** | 15 m/s 47 | 12 m/s 49 | 12.8 m/s (46.8 km/h) 53 |
| **Governing Energy Model** | U-shaped Power Curve | U-shaped Power Curve | Breguet Efficiency (η × L/D) |

## 5. Commercial Mission Planning Software: Algorithms and Simplifications

Mission planning software serves as the critical interface between the desired photogrammetric outcomes of the operator and the physical execution of the flight by the autopilot. However, the internal algorithms used to estimate flight time and energy consumption vary wildly in accuracy, often relying on vast geometric and kinematic simplifications of the physical world.

### 5.1 Time and Energy Estimation in Open-Source Stacks (ArduPilot, PX4)

At the firmware level, open-source flight controllers like ArduPilot and PX4 handle the continuous-time execution of discrete mission waypoints.

In basic waypoint navigation, both stacks historically calculate the Estimated Time of Arrival (ETA) by simply dividing the linear 3D distance between waypoints by the commanded cruise speed (ETA = distance / V_cruise).4 This naive arithmetic completely disregards the acceleration and deceleration limitations of the airframe, the centripetal velocity losses during coordinated turns, and the vector addition of the prevailing wind field.4 Consequently, real-world flight times routinely exceed basic software estimates by 10% to 20%, resulting in premature battery depletion alarms and aborted missions mid-survey.4

To address the impact of wind, advanced control logic has been integrated into recent firmware updates. For fixed-wing navigation, PX4 employs Nonlinear Path Following Guidance (NPFG) algorithms, which supersede legacy L1 controllers.15 The NPFG logic dynamically adjusts the airspeed setpoint based on real-time wind estimation derived from the Extended Kalman Filter (EKF2).15 The EKF2 utilizes an internal parameter, EKF2_WIND_NOISE, to fuse GPS velocity, IMU acceleration, and pitot tube airspeed to estimate the wind state.15 The NPFG features modes such as *Wind excess regulation* (matching airspeed to wind speed) and *Minimum forward ground speed maintenance* (increasing airspeed to ensure positive ground progression against strong headwinds).15 While this logic ensures the drone safely maintains the geometric track, pushing the airspeed higher to overcome a headwind fundamentally violates the constant-velocity assumption used by the ground control station's initial time estimate, thereby accelerating battery drain beyond predicted limits.

### 5.2 Ground Control Station Implementations (QGroundControl, UgCS)

Ground Control Stations (GCS) such as QGroundControl (QGC) and SPH Engineering's UgCS abstract the mathematical complexity of flight planning into user-friendly graphical interfaces, but introduce their own simplifications.

UgCS specializes in photogrammetric routing and automated terrain following. When a user specifies a target GSD and selects a camera profile, the software explicitly calculates and locks the altitude Above Ground Level (AGL) using the simplified pinhole camera model equation GSD = (AGL × p) / f, where f is the lens focal length and p is the sensor pixel pitch.5 The software automates the generation of the boustrophedon grid and calculates the required number of parallel passes based on the requested frontal and lateral overlap, incorporating underlying Digital Elevation Models (DEM) to dynamically adjust altitude.5 However, the time estimation module in UgCS models "perfect flight conditions"—essentially treating the UAV as a particle operating in a vacuum with instantaneous waypoint transitions.59 It does not incorporate the battery's non-linear voltage discharge curve or the massive energy penalty of accelerating a heavy payload out of every turn.59

QGroundControl offers similar grid-generation tools and integrates directly with PX4 and ArduPilot via the MAVLink protocol.60 While QGC calculates total trajectory length and estimates makespan, its handling of turns is defined by an acceptance radius parameter, such as NAV_ACC_RAD.61 When a drone enters this radius around a waypoint, the flight controller "accepts" the waypoint and rounds the corner toward the next destination. While this prevents the drone from coming to a complete stop and minimizes aggressive deceleration, the mathematical simplification in the planning phase assumes a straight-line distance to the exact waypoint. It neglects the arc length of the actual curved trajectory and the deceleration required to navigate the corner without overshooting the adjacent flight line.62

## 6. Turnaround Pattern Optimization Between Parallel Flight Lines

The overall geometric efficiency of a photogrammetric survey mission relies heavily on how the drone transitions between the parallel data-acquisition flight lines. For fixed-wing aircraft, if the lateral distance d between two parallel lines is less than twice the Minimum Turn Radius (R_min) of the aircraft, a standard U-turn is mathematically and physically impossible without violating the maximum bank angle constraints imposed by the flight controller.24

### 6.1 Mathematical Modeling of Course Reversals

When d < 2 × R_min, the path planning algorithm must generate a complex course reversal maneuver to align the aircraft with the subsequent sweep. These maneuvers are derived from manned aviation instrument approach paradigms and robotic path-planning theories:

- **Dubins Paths:** A fundamental mathematical framework that proves the shortest possible path between two oriented points for a vehicle with a constrained turning radius consists exclusively of maximum-curvature arcs (C) and straight-line segments (S).24 For parallel line transitions in tight photogrammetric grids, the optimal Dubins paths are typically the RLR (Right-Left-Right) or LSL (Left-Straight-Left) geometries.24 While optimal in terms of absolute path length, the instantaneous transition from a maximum right bank to a maximum left bank induces aggressive roll rates. This challenges the flight controller, increases induced drag, and can cause altitude loss.65

- **Racetrack Patterns:** Analogous to an ICAO holding pattern, the aircraft turns 180 degrees away from the next flight line, flies straight outbound for a designated time or distance, and then executes a second 180-degree standard rate turn to align perfectly with the new line.66 While ensuring smooth, wings-level alignment well before the camera begins triggering, this method incurs massive non-productive flight distance and temporal penalties, devastating the overall energy efficiency of the mission.67

- **Procedure Turns (Teardrop/Bulb):** To minimize the distance penalty of the racetrack, the procedure turn requires the aircraft to fly past the end of the survey line, turn outward at a specific angle (e.g., 45 degrees or 80 degrees), fly straight for a set time, and then execute a standard rate return turn (e.g., 180 or 260 degrees) to intercept the next parallel line.66 This is mathematically modeled to minimize the airspace used outside the survey polygon while ensuring the aircraft achieves wings-level stabilization before entering the next productive flight line.67

| **Turnaround Maneuver** | **Geometric Condition** | **Trajectory Characteristics** | **Operational Energy Penalty** |
| --- | --- | --- | --- |
| **U-Turn (****-turn)** |  | Single 180° continuous arc at constant bank angle. | Low. Minimal time spent outside the survey boundary; maintains kinetic energy. |
| **Dubins RLR / LSL** | ~2.5 × R_min | Interlocking circles or sharp straight-to-arc transitions. | Medium. Shortest distance, but rapid roll reversals spike drag and require thrust compensation. |
| **Teardrop (Bulb)** | ~3 × R_min + overshoot | Outward divergence followed by a sweeping return arc. | High. Requires significant overshoot of the survey polygon and extended maneuvering. |
| **Racetrack** | 2π × R_min + d_straight | Two 180° arcs separated by long straight segments. | Very High. Maximum non-productive distance traveled; highly inefficient for continuous surveying. |

### 6.2 Wind Trochoids and Minimum Distance Optimization

The presence of a uniform wind field drastically complicates the geometry of these turns. A circular turn executed by a UAV in an air-relative frame is distorted into a trochoidal track over the ground due to the continuous lateral shift induced by the wind vector.13 Path planning algorithms must account for this shift to prevent the UAV from drifting past the target flight line during the turnaround, which would force the flight controller to initiate aggressive, energy-draining corrective maneuvers upon re-entry.

To minimize the energy expended during turnarounds, modern decomposition-based mission planners (such as those studied extensively in AIAA literature) actively optimize the survey grid generation.38 By rotating the flight lines such that they run parallel to the longest axis of the survey polygon, the software minimizes the absolute number of turns required to cover the area.8

Furthermore, instead of flying contiguous parallel lines in sequential order (1, 2, 3, 4), advanced optimization algorithms interleave the flight lines (e.g., flying lines 1, 3, 2, 4).1 By skipping adjacent lines, the lateral distance  between the exit of line 1 and the entry of line 3 becomes large enough to satisfy the critical condition d_skip > 2 × R_min. This allows the fixed-wing UAV to execute simple, continuous-arc U-turns at the boundaries, entirely bypassing the need for energy-intensive teardrop or racetrack maneuvers. This interleaving strategy drastically reduces total flight time, maximizes the aerodynamic efficiency of the turn, and mitigates the complex wind-vector corrections required during multi-stage procedure turns.

## 7. Conclusions

The rigorous estimation of flight time and energy consumption for photogrammetric UAV missions requires an interdisciplinary approach that bridges kinematic trajectory planning, aerodynamic performance modeling, and optical physics constraints. The findings of this analysis indicate that simple linear calculations, commonly deployed in commercial software such as Mission Planner and UgCS, systematically fail to account for the dynamic realities of acceleration phases, aerodynamic drag penalties, and the complex geometry of turnaround maneuvers.

For multirotor platforms, overcoming these algorithmic deficiencies requires integrating the U-shaped power curve dynamics into the planning phase, recognizing that the optimal flight speed scales directly with the square root of the payload mass. However, establishing this theoretical aerodynamic efficiency is frequently hindered by optical constraints, where the necessity to avoid motion blur artificially limits the maximum flight speed, forcing multirotors to operate at suboptimal aerodynamic efficiencies. In contrast, fixed-wing endurance is governed by the Breguet range principles, where efficiency relies heavily on lift-to-drag optimization and minimizing the parasitic drag induced by crosswinds or aggressive Dubins-curve course reversals.

Future advancements in UAV survey reliability depend upon the integration of these high-fidelity aerodynamic and power models directly into ground control station algorithms. By transitioning from purely geometric path planning to energy-aware, physics-based simulations that account for localized wind vectors, vehicle kinematics, and battery discharge non-linearities, operators can maximize areal coverage, ensure safe mission termination, and fully exploit the capabilities of both rotary and fixed-wing autonomous platforms.

#### Works cited

- Kinematic Orienteering Problem with Time-Optimal Trajectories for Multirotor UAVs - arXiv.org, accessed March 24, 2026, [https://arxiv.org/pdf/2211.16837](https://arxiv.org/pdf/2211.16837)

- Advances in UAV Path Planning: A Comprehensive Review of Methods, Challenges, and Future Directions - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2504-446X/9/5/376](https://www.mdpi.com/2504-446X/9/5/376)

- Optimization of Flight Planning for Orthomosaic Generation Using Digital Twins and SITL Simulation - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2504-446X/9/6/407](https://www.mdpi.com/2504-446X/9/6/407)

- Uncertainty Quantification of Expected Time-of-Arrival in UAV Flight Trajectory, accessed March 24, 2026, [https://ntrs.nasa.gov/api/citations/20205009946/downloads/UQ_trajectory_AIAA_aviation2021.pdf](https://ntrs.nasa.gov/api/citations/20205009946/downloads/UQ_trajectory_AIAA_aviation2021.pdf)

- UgCS: Drone Photogrammetry Tool | Reliable UAV Survey Planning - SPH Engineering, accessed March 24, 2026, [https://www.sphengineering.com/news/ugcs-photogrammetry-tool-for-uav-land-survey-missions](https://www.sphengineering.com/news/ugcs-photogrammetry-tool-for-uav-land-survey-missions)

- Optimization of UAV Flight Parameters for Urban Photogrammetric Surveys: Balancing Orthomosaic Visual Quality and Operational Efficiency - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2504-446X/9/11/753](https://www.mdpi.com/2504-446X/9/11/753)

- Game-Theoretic Coordination For Time-Critical Missions of UAV Systems - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/390772781_Game-Theoretic_Coordination_For_Time-Critical_Missions_of_UAV_Systems](https://www.researchgate.net/publication/390772781_Game-Theoretic_Coordination_For_Time-Critical_Missions_of_UAV_Systems)

- Fixed Wing UAV Survey Coverage Path Planning in Wind for Improving Existing Ground Control Station Software - SciSpace, accessed March 24, 2026, [https://scispace.com/pdf/fixed-wing-uav-survey-coverage-path-planning-in-wind-for-4zckppdqnn.pdf](https://scispace.com/pdf/fixed-wing-uav-survey-coverage-path-planning-in-wind-for-4zckppdqnn.pdf)

- Dynamic Bearing–Angle for Vision-Based UAV Target Motion Analysis - MDPI, accessed March 24, 2026, [https://www.mdpi.com/1424-8220/25/14/4396](https://www.mdpi.com/1424-8220/25/14/4396)

- Trajectory Planning of Unmanned Aerial Vehicles in Complex Environments Based on Intelligent Algorithm - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2504-446X/9/7/468](https://www.mdpi.com/2504-446X/9/7/468)

- Effective mission planning for fixed-wing Unmanned Aerial Vehicle - CEUR-WS.org, accessed March 24, 2026, [https://ceur-ws.org/Vol-3981/paper07.pdf](https://ceur-ws.org/Vol-3981/paper07.pdf)

- Fixed-wing UAV Kinematics Model using Direction Restriction for Formation Cooperative Flight - Semantic Scholar, accessed March 24, 2026, [https://pdfs.semanticscholar.org/89f6/6585e7bc8fa38cb62361000d494e9c7eb20b.pdf](https://pdfs.semanticscholar.org/89f6/6585e7bc8fa38cb62361000d494e9c7eb20b.pdf)

- Time-Constrained Extremal Trajectory Design for Fixed-Wing Unmanned Aerial Vehicles in Steady Wind | Journal of Guidance, Control, and Dynamics - Aerospace Research Central, accessed March 24, 2026, [https://arc.aiaa.org/doi/10.2514/1.G003353](https://arc.aiaa.org/doi/10.2514/1.G003353)

- (PDF) Wind-Aware Trajectory Planning for Fixed-Wing Aircraft in Loss of Thrust Emergencies - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/329564121_Wind-Aware_Trajectory_Planning_for_Fixed-Wing_Aircraft_in_Loss_of_Thrust_Emergencies](https://www.researchgate.net/publication/329564121_Wind-Aware_Trajectory_Planning_for_Fixed-Wing_Aircraft_in_Loss_of_Thrust_Emergencies)

- PX4 Fixed Wing Mission Acceptance Radius, accessed March 24, 2026, [https://discuss.px4.io/t/px4-fixed-wing-mission-acceptance-radius/34693](https://discuss.px4.io/t/px4-fixed-wing-mission-acceptance-radius/34693)

- (PDF) Acceleration-Aware Path Planning with Waypoints - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/356669378_Acceleration-Aware_Path_Planning_with_Waypoints](https://www.researchgate.net/publication/356669378_Acceleration-Aware_Path_Planning_with_Waypoints)

- 4.3 Projectile Motion - University Physics Volume 1 | OpenStax, accessed March 24, 2026, [https://openstax.org/books/university-physics-volume-1/pages/4-3-projectile-motion](https://openstax.org/books/university-physics-volume-1/pages/4-3-projectile-motion)

- Enhancing Multirotor Drone Efficiency: Exploring Minimum Energy Consumption Rate of Forward Flight under Varying Payload - arXiv, accessed March 24, 2026, [https://arxiv.org/pdf/2501.03102](https://arxiv.org/pdf/2501.03102)

- Model-Based Development of Multirotor UAV Power Profiles for Performance Investigation of Different Flight Missions - Purdue e-Pubs, accessed March 24, 2026, [https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1245&context=jate](https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1245&context=jate)

- (PDF) Time-Coordinated Control for Unmanned Aerial Vehicle Swarm Cooperative Attack on Ground-Moving Target - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/334854850_Time-Coordinated_Control_for_Unmanned_Aerial_Vehicle_Swarm_Cooperative_Attack_on_Ground-Moving_Target](https://www.researchgate.net/publication/334854850_Time-Coordinated_Control_for_Unmanned_Aerial_Vehicle_Swarm_Cooperative_Attack_on_Ground-Moving_Target)

- Turn Radius Banked Interactive Calculator - Firgelli Automations, accessed March 24, 2026, [https://www.firgelliauto.com/es/blogs/calculators/turn-radius-banked-calculator](https://www.firgelliauto.com/es/blogs/calculators/turn-radius-banked-calculator)

- How to find Radius of Turn given groundspeed and bank angle? [duplicate], accessed March 24, 2026, [https://aviation.stackexchange.com/questions/89378/how-to-find-radius-of-turn-given-groundspeed-and-bank-angle](https://aviation.stackexchange.com/questions/89378/how-to-find-radius-of-turn-given-groundspeed-and-bank-angle)

- Turn Performance - Code 7700, accessed March 24, 2026, [https://code7700.com/turn_performance.htm](https://code7700.com/turn_performance.htm)

- (PDF) Dubins path generation for a fixed wing UAV - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/269299811_Dubins_path_generation_for_a_fixed_wing_UAV](https://www.researchgate.net/publication/269299811_Dubins_path_generation_for_a_fixed_wing_UAV)

- A Comprehensive Taxonomy of UAVs: A Comparative Feature Matrix for Hardware, Capabilities, and Payloads - IEEE Xplore, accessed March 24, 2026, [https://ieeexplore.ieee.org/iel8/6287639/11323511/11288985.pdf](https://ieeexplore.ieee.org/iel8/6287639/11323511/11288985.pdf)

- Design & Implementation of an Electric Fixed-wing Hybrid VTOL UAV for Asset Monitoring - SciELO, accessed March 24, 2026, [https://www.scielo.br/j/jatm/a/M7pKrSRj7nv6H8FhYqt6QKm/?format=pdf&lang=en](https://www.scielo.br/j/jatm/a/M7pKrSRj7nv6H8FhYqt6QKm/?format=pdf&lang=en)

- Effective L/D: A Theoretical Approach to the Measurement of Aero-Structural Efficiency in Aircraft Design, accessed March 24, 2026, [https://ntrs.nasa.gov/api/citations/20160006027/downloads/20160006027.pdf](https://ntrs.nasa.gov/api/citations/20160006027/downloads/20160006027.pdf)

- How to correct the Bréguet range equation taking into account the fuel flow rate of the aircraft ? - eucass, accessed March 24, 2026, [https://www.eucass.eu/doi/EUCASS2019-0117.pdf](https://www.eucass.eu/doi/EUCASS2019-0117.pdf)

- Range and Endurance - Aircraft Flight Mechanics by Harry Smith, PhD, accessed March 24, 2026, [https://aircraftflightmechanics.com/AircraftPerformance/RangeandEndurance.html](https://aircraftflightmechanics.com/AircraftPerformance/RangeandEndurance.html)

- A New Approach to Calculating Endurance in Electric Flight and Comparing Fuel Cells and Batteries - IRIS, accessed March 24, 2026, [https://iris.unisalento.it/retrieve/381f5170-969e-4d01-acaa-46e94e3fabea/A%20New%20Approach%20to%20Calculating%20Endurance%20in%20Electric%20Flight%20and%20Comparing%20Fuel%20Cells%20and%20Batteries%20_%20postprint.pdf](https://iris.unisalento.it/retrieve/381f5170-969e-4d01-acaa-46e94e3fabea/A%20New%20Approach%20to%20Calculating%20Endurance%20in%20Electric%20Flight%20and%20Comparing%20Fuel%20Cells%20and%20Batteries%20_%20postprint.pdf)

- (PDF) Range and Endurance Estimates for Battery-Powered Aircraft - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/269567470_Range_and_Endurance_Estimates_for_Battery-Powered_Aircraft](https://www.researchgate.net/publication/269567470_Range_and_Endurance_Estimates_for_Battery-Powered_Aircraft)

- Flight Range & Endurance – Introduction to Aerospace Flight Vehicles - Eagle Pubs, accessed March 24, 2026, [https://eaglepubs.erau.edu/introductiontoaerospaceflightvehicles/chapter/flight-range-endurance/](https://eaglepubs.erau.edu/introductiontoaerospaceflightvehicles/chapter/flight-range-endurance/)

- On the Range Equation for Hybrid-Electric Aircraft - University of Malta, accessed March 24, 2026, [https://www.um.edu.mt/library/oar/bitstream/123456789/114399/1/On_the_range_equation_for_a_hybrid-electric_aircraft%282023%29.pdf](https://www.um.edu.mt/library/oar/bitstream/123456789/114399/1/On_the_range_equation_for_a_hybrid-electric_aircraft%282023%29.pdf)

- Preventing Motion Blur in Drone Photogrammetry Flights, accessed March 24, 2026, [https://www.hammermissions.com/post/preventing-motion-blur-in-drone-photogrammetry-flights](https://www.hammermissions.com/post/preventing-motion-blur-in-drone-photogrammetry-flights)

- UAV GSD & Mapping Calculator - Fadron, accessed March 24, 2026, [https://fadron.com/gsd-calculator/](https://fadron.com/gsd-calculator/)

- Preventing motion blur in drone mapping - Richard Hann - NTNU, accessed March 24, 2026, [https://www.ntnu.no/blogger/richard-hann/2021/10/07/preventing-motion-blur-in-drone-mapping/](https://www.ntnu.no/blogger/richard-hann/2021/10/07/preventing-motion-blur-in-drone-mapping/)

- Range, Endurance, and Optimal Speed Estimates for Multicopters - ZORA, accessed March 24, 2026, [https://www.zora.uzh.ch/server/api/core/bitstreams/f8cb1ecf-91fc-4237-be26-bec1f2063dfd/content](https://www.zora.uzh.ch/server/api/core/bitstreams/f8cb1ecf-91fc-4237-be26-bec1f2063dfd/content)

- Decomposition-based mission planning for fixed-wing UAVs surveying in wind - Loughborough University Research Repository, accessed March 24, 2026, [https://repository.lboro.ac.uk/articles/journal_contribution/Decomposition-based_mission_planning_for_fixed-wing_UAVs_surveying_in_wind/10318751](https://repository.lboro.ac.uk/articles/journal_contribution/Decomposition-based_mission_planning_for_fixed-wing_UAVs_surveying_in_wind/10318751)

- A Comparative Study on Energy Consumption Models for Drones - DORAS, accessed March 24, 2026, [https://doras.dcu.ie/27277/1/Main.pdf](https://doras.dcu.ie/27277/1/Main.pdf)

- Modelling Power Consumptions for Multi-rotor UAVs - arXiv, accessed March 24, 2026, [https://arxiv.org/pdf/2209.04128](https://arxiv.org/pdf/2209.04128)

- (PDF) Decomposition‐based mission planning for fixed‐wing UAVs surveying in wind, accessed March 24, 2026, [https://www.researchgate.net/publication/337976413_Decomposition-based_mission_planning_for_fixed-wing_UAVs_surveying_in_wind](https://www.researchgate.net/publication/337976413_Decomposition-based_mission_planning_for_fixed-wing_UAVs_surveying_in_wind)

- Design and Performance Study of Small Multirotor UAVs with Adjunctive Folding-Wing Range Extender - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2504-446X/9/12/877](https://www.mdpi.com/2504-446X/9/12/877)

- Enhancing Multirotor Drone Efficiency: Exploring Minimum Energy Consumption Rate of Forward Flight under Varying Payload - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/387767241_Enhancing_Multirotor_Drone_Efficiency_Exploring_Minimum_Energy_Consumption_Rate_of_Forward_Flight_under_Varying_Payload](https://www.researchgate.net/publication/387767241_Enhancing_Multirotor_Drone_Efficiency_Exploring_Minimum_Energy_Consumption_Rate_of_Forward_Flight_under_Varying_Payload)

- [Literature Review] Enhancing Multirotor Drone Efficiency: Exploring Minimum Energy Consumption Rate of Forward Flight under Varying Payload - Moonlight, accessed March 24, 2026, [https://www.themoonlight.io/en/review/enhancing-multirotor-drone-efficiency-exploring-minimum-energy-consumption-rate-of-forward-flight-under-varying-payload](https://www.themoonlight.io/en/review/enhancing-multirotor-drone-efficiency-exploring-minimum-energy-consumption-rate-of-forward-flight-under-varying-payload)

- (PDF) Design optimization of multirotor drones in forward flight - ResearchGate, accessed March 24, 2026, [https://www.researchgate.net/publication/356604555_Design_optimization_of_multirotor_drones_in_forward_flight](https://www.researchgate.net/publication/356604555_Design_optimization_of_multirotor_drones_in_forward_flight)

- Matrice 350 RTK - DJI Enterprise, accessed March 24, 2026, [https://enterprise.dji.com/matrice-350-rtk](https://enterprise.dji.com/matrice-350-rtk)

- DJI M300 RTK Specs - Drone Ops, accessed March 24, 2026, [https://droneops.co.za/dji-m300-rtk-specs/](https://droneops.co.za/dji-m300-rtk-specs/)

- MATRICE 300 RTK - DJI, accessed March 24, 2026, [https://dl.djicdn.com/downloads/matrice-300/20200529/M300_RTK_User_Manual_EN_0604.pdf](https://dl.djicdn.com/downloads/matrice-300/20200529/M300_RTK_User_Manual_EN_0604.pdf)

- News & Case Studies DJI Matrice 350 RTK vs DJI M300 RTK – Key Differences, accessed March 24, 2026, [https://www.ferntech.co.nz/blog/news-case-studies/dji-matrice-350-rtk-vs-dji-300-rtk](https://www.ferntech.co.nz/blog/news-case-studies/dji-matrice-350-rtk-vs-dji-300-rtk)

- DJI Matrice 300 RTK Spotlight - Secrets & Lies - Coptrz, accessed March 24, 2026, [https://coptrz.com/blog/secrets-lies-dji-matrice-300-rtk/](https://coptrz.com/blog/secrets-lies-dji-matrice-300-rtk/)

- DJI Matrice 300 vs. WingtraOne GEN II: mapping drone comparison | Wingtra, accessed March 24, 2026, [https://wingtra.com/best-drones-for-photogrammetry-wingtraone-comparison/dji-matrice-300-vs-wingtraone/](https://wingtra.com/best-drones-for-photogrammetry-wingtraone-comparison/dji-matrice-300-vs-wingtraone/)

- DJI M350 RTK Top Features - Measur Drones, accessed March 24, 2026, [https://measur.ca/blogs/news/dji-m350-rtk-top-features](https://measur.ca/blogs/news/dji-m350-rtk-top-features)

- SenseFly eBee X - Sobek, accessed March 24, 2026, [https://sobekglobal.com/drone/sensefly-ebee-x/](https://sobekglobal.com/drone/sensefly-ebee-x/)

- eBee X | Bezpilotne.sk, accessed March 24, 2026, [http://bezpilotne.sk/wp-content/uploads/2018/09/eBeeX-finalversion-A4_EN_web.pdf](http://bezpilotne.sk/wp-content/uploads/2018/09/eBeeX-finalversion-A4_EN_web.pdf)

- eBee X Series Drone User Manual | PDF | Unmanned Aerial Vehicle | Damages - Scribd, accessed March 24, 2026, [https://www.scribd.com/document/899198997/EBee-X-User-Manual-3-23-3](https://www.scribd.com/document/899198997/EBee-X-User-Manual-3-23-3)

- User Manual: eBee X, accessed March 24, 2026, [https://geomatika-smolcak.hr/wp-content/uploads/2018/10/eBee-X-Drone-User-Manual.pdf](https://geomatika-smolcak.hr/wp-content/uploads/2018/10/eBee-X-Drone-User-Manual.pdf)

- Flight Data Screen Overview — Mission Planner documentation, accessed March 24, 2026, [https://ardupilot.org/planner/docs/mission-planner-ground-control-station.html](https://ardupilot.org/planner/docs/mission-planner-ground-control-station.html)

- Contour Mission Flight Planning of UAV for Photogrammetric in Hillside Areas - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2076-3417/13/13/7666](https://www.mdpi.com/2076-3417/13/13/7666)

- Webinar | UgCS features explained: #2 Automatic Photogrammetry tool - SPH Engineering, accessed March 24, 2026, [https://www.sphengineering.com/news/webinar-ugcs-features-explained-2-automatic-photogrammetry-tool](https://www.sphengineering.com/news/webinar-ugcs-features-explained-2-automatic-photogrammetry-tool)

- Extending QGroundControl for Automated Mission Planning of UAVs - MDPI, accessed March 24, 2026, [https://www.mdpi.com/1424-8220/18/7/2339](https://www.mdpi.com/1424-8220/18/7/2339)

- Analysis of the PX4-Firmware - Technische Hochschule Augsburg, accessed March 24, 2026, [https://www.hs-augsburg.de/homes/beckmanf/dokuwiki/doku.php?id=searchwing-px4-analyse](https://www.hs-augsburg.de/homes/beckmanf/dokuwiki/doku.php?id=searchwing-px4-analyse)

- Mission Mode | PX4 User Guide - GitBook, accessed March 24, 2026, [https://px4.gitbook.io/px4-user-guide/flying/flight_modes/mission](https://px4.gitbook.io/px4-user-guide/flying/flight_modes/mission)

- Development of Fixed-Wing UAV 3D Coverage Paths for Urban Air Quality Profiling - PMC, accessed March 24, 2026, [https://pmc.ncbi.nlm.nih.gov/articles/PMC9143050/](https://pmc.ncbi.nlm.nih.gov/articles/PMC9143050/)

- Dubins paths for resolving a fixed-wing UAV traffic jam, accessed March 24, 2026, [https://ntrs.nasa.gov/api/citations/20250011524/downloads/20250011524-UAV-separation-convex-polygon-AIAA-JAT.pdf?attachment=true](https://ntrs.nasa.gov/api/citations/20250011524/downloads/20250011524-UAV-separation-convex-polygon-AIAA-JAT.pdf?attachment=true)

- Direct Entry Minimal Path UAV Loitering Path Planning - MDPI, accessed March 24, 2026, [https://www.mdpi.com/2226-4310/4/2/23](https://www.mdpi.com/2226-4310/4/2/23)

- Procedure Turn, Hold in Lieu - Code 7700, accessed March 24, 2026, [https://code7700.com/procedure_turn_hold_in_lieu.htm](https://code7700.com/procedure_turn_hold_in_lieu.htm)

- Racetrack and Reversal Procedures Guide | PDF | Aerospace | Aviation - Scribd, accessed March 24, 2026, [https://www.scribd.com/document/390094967/1-16areversalandracetrackprocedures-pdf](https://www.scribd.com/document/390094967/1-16areversalandracetrackprocedures-pdf)

- Racetrack Procedures - Honeywell Aerospace, accessed March 24, 2026, [https://aerospace.honeywell.com/us/en/about-us/news/2019/04/racetrack-procedures](https://aerospace.honeywell.com/us/en/about-us/news/2019/04/racetrack-procedures)