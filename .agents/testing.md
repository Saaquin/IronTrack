# IronTrack — Testing Guide

**Source docs:** 44, 36, 37
Tests are the correctness contract for safety-critical aerial survey math.

## Test Layers [Doc 44]

| Layer | Location | Strategy |
|-------|----------|----------|
| Unit (core engine) | `#[cfg(test)]` per module | proptest round-trips, Karney/NGA/NGS test vectors, Kahan validation |
| Integration (daemon) | `tests/*.rs` | axum-test in-process, tokio-tungstenite WebSocket, Arc<RwLock> contention |
| End-to-end pipeline | `tests/pipeline.rs` | Boundary→3DEP mock→EB+B-spline→GeoPackage→export format verification |
| Golden-file regression | `tests/*.snap` | insta crate — sub-mm geometric deviations flagged on CI |
| Embedded SIL | host x86_64 | embedded-hal-mock UART/GPIO, trigger timing at derivative zero-crossing |
| Embedded HIL | physical MCU | 15K waypoint pagination, 460800 baud DMA, GPIO jitter <100 ns |
| Mock telemetry | daemon mock mode | Virtual aircraft on B-spline + synthetic crosswind, 10 Hz NMEA |
| Field acceptance | Eagle Mapping | Trackair parallel + live flight + ASPRS 2024 compliance |

## Property-Based Testing (proptest) [Doc 44]
Millions of random coordinates across full global domain (±90° lat, ±180° lon).
Round-trip invariants: `from_utm(to_utm(coord))` recovers within ε<1mm.
Helmert round-trip: `inverse_helmert(helmert(coord, epoch))` recovers exactly.
Automatic "shrinking" to minimal reproducible case on failure.

## Authoritative Test Vectors [Doc 44]
| Authority | Dataset | Precision Target |
|-----------|---------|-----------------|
| Karney | GeodTest.dat (500K geodesics) | 15 nm |
| NGA | Gold Data v6.3 | UTM/MGRS/datum edge cases |
| NGS | NCAT / VERTCON | GEOID18 biquadratic validation |

## Kahan Summation Validation [Doc 44]
100K consecutive 1000 Hz dead-reckoning integrations at 80 m/s.
Three-way comparison: naive vs Kahan vs arbitrary-precision oracle (rug crate).
Assert: Kahan error remains O(1), not O(n).

## CI Pipeline [Doc 44]
```yaml
- cargo fmt --check
- cargo clippy -- -D warnings
- cargo test                                    # all unit + integration
- cargo test --release --test geodesy           # release-mode geodesy regression
- cargo test --test terrain_following_validation # SAFETY GATE — never disable
```

Cross-compilation build matrix: x86_64 + aarch64 + thumbv7em-none-eabihf (STM32) +
thumbv6m-none-eabi (RP2040). All four must compile on every PR.
Headless Tauri: xvfb + Playwright. Code coverage: cargo-llvm-cov.
Performance regression: Criterion benchmarks on every PR (>5% slowdown = flag). [Doc 52]

## Eagle Mapping Acceptance Protocol [Docs 44, 36]
Phase 1: Trackair parallel execution + spatial delta analysis (C² vs discontinuous steps).
Phase 2: Live flight with physical trigger controller + sensor payload.
Phase 3: ASPRS 2024 (RMSE only, 30 min checkpoints, NVA/VVA differentiation).
Gate: orthomosaics meet ASPRS class without manual waypoint intervention.

## Anti-Patterns
- Never use `==` on f64. Always `approx`.
- Never `unwrap()` in tests. Use `expect("reason")`.
- Never disable the terrain-following validation suite.
- Fixture realism: DEFLATE+PREDICTOR=3 for TIFF fixtures.
