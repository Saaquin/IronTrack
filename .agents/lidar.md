# LiDAR Flight Planning & System Control Guide

**Source module:** `src/photogrammetry/lidar.rs`
**Source docs:** 13, 41, 45

## How LiDAR Differs from Camera Planning
Camera: GSD (m/px). LiDAR: point density (pts/m²). Completely different math.
Both share: swath width from FOV, terrain-following AGL, lateral overlap.

## Core Equation
```
ρ = PRR × (1 - loss_fraction) / (V_ground × swath_width)    [pts/m²]
```

## LiDAR System Control [Doc 41]
LiDAR sensors fire CONTINUOUSLY from an internal oscillator — no external trigger.
"Triggering" = network-based start/stop data recording at line boundaries via TCP.
The FMS controls the aircraft trajectory; the laser swath follows automatically.

### Manned Systems
| System | PRR | Key Feature | Control Interface |
|--------|-----|-------------|-------------------|
| Leica TerrainMapper-3 | 2 MHz | Gateless 35 MPiA, programmable scan patterns | Leica FlightPro |
| Riegl VQ-1560 II | 4 MHz (dual) | Cross-fire 28° geometry | RiACQUIRE TCP:20002 |
| Optech Galaxy Prime | 1 MHz | SwathTRAK dynamic FOV (40-70% fewer lines) | Optech FMS / Onboard |

### UAV Systems
| System | PRR | Interface | Ecosystem |
|--------|-----|-----------|-----------|
| DJI Zenmuse L2 | 240 kHz | PSDK (DJILidarPointCloudRecord enum) | Closed (DJI Terra) |
| Riegl miniVUX-1UAV | 100-300 kHz | Open GigE + RS-232 + TTL PPS | **Open / agnostic** |
| Livox Mid-360 | Non-repetitive | Ethernet SDK (hex commands) | Open / custom |

### Dual-Trigger Hybrid [Doc 41]
Hybrid payloads (LiDAR + camera): daemon sends TCP start/stop to LiDAR while trigger
controller fires camera GPIO independently. If laptop crashes mid-line: LiDAR continues
logging internally, controller continues firing from local SRAM waypoints.

## Post-Processing Boundary [Doc 41]
IronTrack handles navigation and triggering. SBET, boresight calibration, lever arm,
and point cloud processing are delegated to vendor suites:
HxMap (Leica), RiProcess/RiVLib (Riegl), LMS Pro (Optech), DJI Terra.

## USGS Quality Levels
| QL | Density | NVA (RMSEz) | VVA (RMSEz) |
|----|---------|-------------|-------------|
| QL0 | ≥ 8 pts/m² | ≤ 9.25 cm | ≤ 18.5 cm |
| QL1 | ≥ 8 pts/m² | ≤ 10.0 cm | ≤ 20.0 cm |
| QL2 | ≥ 2 pts/m² | ≤ 10.0 cm | ≤ 20.0 cm |
