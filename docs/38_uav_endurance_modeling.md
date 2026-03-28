# UAV and Fixed-Wing Aerodynamic Power and Endurance Modeling

> **Source document:** 38_UAV-Endurance-Modeling.docx
>
> **Scope:** Complete extraction — all content is unique. Covers multirotor momentum theory, fixed-wing drag polars, propeller advance ratio, LiPo battery electrochemistry, per-segment energy costing, and endurance estimation workflow.

---

## 1. Multirotor Power Model

### 1.1 Hover Power (Momentum Theory)
```
P_hover = (m × g)^1.5 / √(2 × ρ × A)
```
- m = total takeoff mass including payload (kg). Mass raised to 1.5 power — minor payload increases cause exponential power growth.
- ρ = air density (kg/m³). Decreases with altitude — must cross-reference geometric altitude from EGM2008/GEOID18 with atmospheric model.
- A = total rotor disk area (m²). Larger propellers moving air slowly are vastly more efficient than small propellers moving air rapidly (inverse square root relationship).

Profile drag (blade friction) adds a baseline power sink scaling with the cube of rotor angular velocity.

### 1.2 Forward Flight Power
Total = induced + parasite + climb:
- **Induced power** decreases with forward speed (translational lift — higher mass flow through rotor disk).
- **Parasite power** `P_parasite = 0.5 × ρ × Cd × A_frontal × V³` — scales cubically with TAS, quickly overwhelms translational lift savings.
- **Climb power** `P_climb = m × g × V_z` — additive to forward flight power.

### 1.3 Speed-vs-Power Curve (U-Shaped)
- **Minimum power speed (V_mp):** Lowest point on the curve. Maximum endurance (longest flight time).
- **Maximum range speed:** Tangent from origin to curve. Greatest distance per joule. Ideal for large-scale mapping.
- Adding payload shifts the entire curve upward and right — heavier aircraft must fly faster for optimal efficiency.

### 1.4 Wind Effects
Aerodynamic power is a function of TAS, not ground speed. For constant GS over the survey grid:
- Headwind: TAS = GS + V_wind → parasite power scales cubically → drastically accelerated battery depletion.
- Tailwind: TAS = GS - V_wind → slide back down the power curve → energy saved.
- IronTrack must integrate asymmetric energy costs of upwind vs downwind legs.

---

## 2. Fixed-Wing Power Model

### 2.1 Drag Polar
```
C_D = C_D0 + C_L² / (π × e × AR)
```
- C_D0 = zero-lift (parasite) drag coefficient. Baseline drag of fuselage, antennas, payload.
- C_L = lift coefficient. Proportional to angle of attack.
- AR = aspect ratio (b²/S). High AR wings (long, slender) reduce induced drag — optimal for endurance.
- e = Oswald efficiency factor (0.70–0.85 for conventional UAVs). Measures deviation from ideal elliptical lift distribution.

### 2.2 Power Required for Level Flight
```
P_required = 0.5 × ρ × V³ × S × C_D0  +  (2 × W²) / (ρ × V × π × b² × e)
              \_________________________/     \__________________________________/
               Parasite power (∝ V³)           Induced power (∝ 1/V)
```
- First term dominates at high speed. Second term dominates at low speed.
- Where parasite = induced drag → minimum drag speed (V_md) → maximum L/D → maximum range.
- Minimum power speed (V_mp) ≈ 0.76 × V_md → maximum endurance.

### 2.3 Propeller Efficiency and Advance Ratio
Propeller efficiency η is NOT a constant. It varies with advance ratio:
```
J = V_TAS / (n × D)
```
Where n = revolutions/second, D = propeller diameter.

- J = 0 (static): η = 0 (no forward motion).
- J increases → η climbs to peak.
- J too high (steep dive, massive tailwind) → windmill brake state → η plummets.

Model using empirical 3rd-order polynomial fits for thrust coefficient (Ct) and power coefficient (Cp) from UIUC wind tunnel data (APC Slow Flyer propellers):
```
Ct(J) = a₀ + a₁J + a₂J² + a₃J³
Cp(J) = b₀ + b₁J + b₂J² + b₃J³
η = J × Ct / Cp
```

---

## 3. Battery Discharge Model

### 3.1 Peukert Effect
Usable capacity decreases as discharge rate increases:
```
t_actual = t_rated × (C_rated / I_actual)^k
```
- k (Peukert constant): ~1.05 for modern LiPo/Li-ion (vs 1.2–1.6 for lead-acid).
- Nearly negligible during steady-state cruise, but **highly relevant during transient high-load maneuvers** (vertical takeoff, aggressive climb, tight Dubins turns).
- IronTrack must incrementally penalize effective capacity during high-current phases.

### 3.2 Thermal Dependencies
- **Cold (near 0°C):** Sluggish ion mobility → severe internal resistance spike → extreme voltage sag under load. Battery holds full charge but cannot deliver it.
- **Hot (>45°C ambient or >60°C internal):** Accelerated SEI layer destruction → permanent internal resistance increase → potential thermal runaway.
- IronTrack planning UI must accept expected environmental temperature to de-rate predicted capacity.

### 3.3 Voltage Sag and Internal Resistance
```
V_sag = I × R_internal
V_terminal = V_open_circuit - V_sag - V_polarization
```
- R_internal increases with age, cold temperature, and low SoC.
- A current spike during a gust or rapid climb at 40% SoC can breach the ESC's low-voltage cutoff (3.2–3.5V/cell) → uncommanded emergency landing or total brownout.
- IronTrack's model must estimate dynamic V_sag to ensure transient maneuvers don't trigger hardware LVC.

### 3.4 Reserve Requirements — IronTrack ENFORCES 20%
The LiPo discharge curve is flat through the middle but exhibits a **sharp non-linear voltage cliff in the final 15–20% SoC**. Drawing high current in this depleted state amplifies voltage sag severity, virtually guaranteeing LVC breach.

**IronTrack treats the final 20% of battery capacity as mathematically inaccessible.** If a flight plan intrudes upon this reserve, the system generates a **blocking warning** requiring explicit mitigation (shorten lines, add battery swaps) before export is permitted.

---

## 4. Energy Cost Per Flight Segment

### 4.1 Straight-and-Level (Data Acquisition)
```
E_line = P_required(V_TAS) × (distance / V_GS)
```
Wind vector resolved against heading to determine TAS. Asymmetric costs: upwind legs consume drastically more energy than downwind legs.

### 4.2 Turn Segments (Load Factor Penalty)
Bank angle → load factor → induced drag penalty:

| Bank Angle | Load Factor n | Induced Drag Penalty (n²) |
|---|---|---|
| 0° (straight) | 1.00 | 1.0× |
| 30° | 1.15 | 1.33× |
| 45° | 1.41 | 2.0× |
| 60° | 2.00 | **4.0×** |

A 60° bank quadruples induced drag. IronTrack's Dubins/clothoid turn limits (max bank angle from v0.3) serve a dual purpose: stabilizing gimbals AND capping exponential drag spikes that cause battery sag.

### 4.3 Climb Segments
```
P_climb = m × g × V_z    (additive to forward flight power)
```
Universally the highest sustained power-draw phase. Peukert penalty coefficients must be applied during prolonged ascents.

### 4.4 Descent Segments (Regenerative Recovery)
In advanced electric propulsion, descent can recapture energy via regenerative braking (FOC motor drivers invert brushless motors to act as alternators). Flight tests show ~6.7% energy recovery during approach patterns. Fixed-wing platforms benefit more than multirotors (which still need continuous downward thrust).

IronTrack integrates a platform-specific regenerative efficiency coefficient (η_regen) to credit energy recovery during descent, subtracting from total mission cost.

---

## 5. Endurance Estimation Workflow

### 5.1 Total Mission Energy
```
E_total = Σ(E_line[i] + E_turn[i] + E_climb[i] - E_regen[i])
```
Computed on rayon threads (synchronous, isolated from Axum daemon). **Kahan summation mandatory** on f64 to prevent floating-point drift over thousands of waypoints. Continuously factors payload mass, localized air density (from datum-tagged altitude), and projected wind vectors.

### 5.2 Capacity vs Safety Factor
```
feasible = E_total_adjusted ≤ 0.80 × E_battery_nominal
```
E_total_adjusted includes Peukert penalties during high-current phases and temperature de-rating. If the adjusted energy exceeds 80% of nominal capacity → mission flagged as compromised.

### 5.3 Operator Presentation
Rendered per ARINC 661 / AC 25-11B color standards on the React planning UI and Tauri glass cockpit:

```
Estimated endurance: 42 minutes. Mission requires: 38 minutes.
Reserve: 10.5%. WARNING: Reserve below 20%.
```

Amber/red warning per alert hierarchy. The operator is informed that while the battery holds enough absolute energy, the depth of discharge will trigger severe voltage sag on the final approach, drastically increasing LVC crash risk.

---

## 6. IronTrack Implementation Notes

### PlatformDynamics Struct
```rust
pub struct PlatformDynamics {
    pub mass_kg: f64,
    pub drag_coefficient: f64,
    pub frontal_area_m2: f64,
    pub wing_area_m2: Option<f64>,        // Fixed-wing only
    pub wingspan_m: Option<f64>,           // Fixed-wing only (for AR)
    pub oswald_efficiency: Option<f64>,    // Fixed-wing only (0.70-0.85)
    pub rotor_disk_area_m2: Option<f64>,   // Multirotor only
    pub propeller_diameter_m: Option<f64>, // For advance ratio
    pub battery_capacity_wh: f64,
    pub battery_peukert_k: f64,            // ~1.05 for LiPo
    pub propulsive_efficiency: f64,        // 0.0-1.0, static fallback
    pub regen_efficiency: f64,             // 0.0-0.10 typical
}
```

### CLI Flags
```
--platform-mass <kg>
--platform-drag-coeff <Cd>
--platform-frontal-area <m²>
--battery-capacity <Wh>
--battery-peukert <k>            (default 1.05)
--ambient-temp <°C>              (for thermal de-rating)
```

### Architecture Constraint
All energy calculations are synchronous rayon work. If called from Axum endpoints, wrap in `tokio::task::spawn_blocking`.
