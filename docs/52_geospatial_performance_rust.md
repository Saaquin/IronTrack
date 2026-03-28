# Rust Performance Optimization for Geospatial Processing

> **Source document:** 52_Geospatial-Performance-Rust.docx
>
> **Scope:** Complete extraction — all content is unique. Covers SIMD vectorization (AVX2 f64x4 for Karney geodesics), rayon work-stealing parallelism with granularity tuning, memory-mapped DEM access (memmap2, page faults, thrashing), GeoPackage SQLite write batching, Profile-Guided Optimization (PGO), and Criterion benchmarking for CI regression.

---

## 1. SIMD Vectorization for Geodesic Batch Operations

### Why Auto-Vectorization Fails for Karney's Algorithm
LLVM auto-vectorization requires provably independent iterations, non-aliasing memory, predictable loop counts. Karney's inverse geodesic is iterative with complex trig, nested branching, and data-dependent loop exits → LLVM cannot safely reconstruct a unified control flow graph for packed execution. Even with `-C opt-level=3 -C target-cpu=native`, geographiclib-rs executes scalar f64 instructions.

### Manual AVX2 SIMD: f64x4
256-bit ymm registers hold exactly 4 × f64. Via `std::arch` intrinsics or `core::simd` portable module, load 4 independent waypoints into one register → 4 concurrent Karney distance calculations using `vaddpd`/`vmulpd`.

**Requires SoA memory layout:** Contiguous latitude array + contiguous longitude array → aligned block loads into ymm registers. AoS layout requires expensive scatter/gather → defeats the purpose.

**Lane masking:** Karney converges at different rates per coordinate pair. Use bitwise execution masks to disable finished lanes while slower lanes continue.

| Strategy | Layout | Register | Parallel Elements | Bottleneck |
|---|---|---|---|---|
| Default scalar | AoS | 64-bit xmm | 1 f64 | Instruction dispatch |
| LLVM auto-vectorized | Unpredictable | Variable | 1–2 f64 | Branch divergence |
| **Manual AVX2** | **SoA** | **256-bit ymm** | **4 f64x4** | Register spilling, lane masking |

### Trigonometric Approximations: Polynomials > LUTs
- **LUTs:** High-precision f64 LUT exceeds L1/L2 cache → cache misses cost 200–400 cycles. SIMD scatter/gather for non-contiguous indices destroys throughput.
- **Polynomials (minimax/Chebyshev):** Execute entirely in registers via FMA instructions. 2–4× faster than LUTs.
- **fast_math REJECTED:** Enables `-ffast-math` → reorders FP operations, assumes no NaN/Inf, flushes subnormals. **Destroys Kahan summation guarantees.** IronTrack must use bounded, controlled minimax polynomial routines — never global fast_math flags.

---

## 2. Rayon Parallelism Patterns

### Natural Data Independence in IronTrack
- **Flight line generation:** Each line in a concave boundary is mathematically independent → `par_iter()` distributes Elastic Band + B-spline Jacobian calculations across all cores.
- **DEM interpolation:** Each waypoint lookup against a static read-only elevation grid is stateless → parallel map-reduction across waypoint arrays.

### Work-Stealing Scheduler (Cilk-Inspired)
- Global thread pool: one worker per logical CPU core.
- Each worker has a private double-ended queue (deque).
- Worker splits payload into halves, pushes sub-tasks to local deque, pops from bottom to execute.
- Idle workers "steal" from other deques (opposite end) → dynamic load balancing.
- Lock-free stealing, but atomic operations + memory barriers + potential futex calls create overhead.

### Task Granularity: with_min_len()
If task granularity is too fine (single FP addition), synchronization overhead > computation time → parallelism hurts performance and wastes battery.

| Task Type | Math Complexity | Recommended min_len |
|---|---|---|
| Simple arithmetic | Extremely low | > 10,000 items |
| DEM bicubic interpolation | Moderate | > 1,000 items |
| Elastic Band matrix solver | High | < 100 items |

**IronTrack mandate:** `with_min_len(1024)` for waypoint evaluation pipelines. Each worker processes a substantial contiguous memory block → leverages L1 cache spatial locality while preventing scheduler futex drowning.

---

## 3. Memory-Mapped DEM Access on Constrained Devices

### The Problem
2 GB GeoTIFF on a 4 GB Raspberry Pi → loading into heap = instant OOM (after kernel, Tauri WebView, Tokio buffers consume available RAM).

### memmap2 Solution
`mmap` system call reserves 2 GB in the 64-bit virtual address space without copying data to RAM. Physical pages loaded on-demand when accessed (page faults). Initial operation: near-instantaneous, zero physical memory.

### Page Fault Physics
- Thread accesses unmapped address → hardware interrupt (major page fault).
- CPU suspends thread, context-switches to kernel, fetches 4 KB page from disk, updates page tables, resumes thread.
- OS "readahead" heuristic prefetches sequential pages (assumes linear access).

### Thrashing on Random Access
If waypoint evaluation jumps sporadically across the 2 GB grid → major page fault per lookup. On Pi's SD card: millisecond latency per fault. Limited 4 GB RAM → kernel violently evicts older pages. System enters continuous disk I/O (thrashing) → destroys geodetic math performance AND starves Tokio's NMEA telemetry parsing.

### Mitigation: R-Tree Geographic Presorting
Before mmap traversal, sort waypoint evaluation order using the GeoPackage R-tree spatial index to group geographically proximate waypoints. This transforms random access into near-sequential access → maximizes OS readahead effectiveness → minimizes page faults → prevents thrashing.

---

## 4. GeoPackage SQLite Write Performance

### Bulk Insertion: Transaction Batching
SQLite's default auto-commit mode wraps each INSERT in an individual transaction → triggers a full fsync to disk per row. For thousands of waypoints: catastrophic I/O bottleneck.

**Mandate:** Wrap bulk insertions in explicit `BEGIN TRANSACTION` / `COMMIT` blocks. All writes accumulate in the WAL journal → single fsync at commit. Orders of magnitude faster.

### WAL Mode vs DELETE Mode
| Feature | DELETE (rollback journal) | WAL (Write-Ahead Log) |
|---|---|---|
| Writer blocks readers | **Yes** (exclusive lock) | **No** (snapshot isolation) |
| Multiple concurrent readers | Limited | **Unlimited** |
| Write throughput | Lower (full page copy) | **Higher** (append-only) |
| Recovery | Rollback journal | WAL + wal-index |

**WAL mandatory for IronTrack:** The daemon writes trigger events at high frequency while the cockpit display simultaneously reads telemetry. DELETE mode would freeze the pilot's instruments during writes.

---

## 5. Profile-Guided Optimization (PGO)

### Instrumentation Phase
Compile with `cargo-pgo` instrumentation flags → binary writes execution profile data (.profdata) during a representative workload (e.g., generating flight plans for a complex mountainous boundary, running mock telemetry at 10 Hz).

### Optimization Phase
Recompile with the profile data → LLVM uses actual branch frequency, hot/cold function data, and call graph weights to:
- Inline hot functions aggressively.
- Optimize branch prediction hints for the actual workload.
- Layout code to maximize instruction cache locality.

### AArch64 Cross-Compilation Challenge
PGO profiles are architecture-specific. Profiling on x86 and applying to an ARM64 binary (Raspberry Pi) produces suboptimal results. Must run the instrumented binary ON the target hardware (Pi 5) or in a representative QEMU environment to generate valid ARM64 profiles.

---

## 6. Benchmarking and CI Regression Analysis

### Criterion Micro-Benchmarking
The `criterion` crate provides statistically rigorous benchmarking:
- Multiple iterations with warm-up phases.
- Statistical analysis (mean, median, confidence intervals).
- Automatic detection of performance regressions between Git commits.

**Key benchmarks for IronTrack CI:**
- Karney inverse geodesic: single-point and batch (1M waypoints).
- DEM bicubic interpolation throughput.
- B-spline trajectory generation for reference boundary.
- GeoPackage bulk insertion (10K waypoints).
- WebSocket telemetry broadcast latency (N concurrent clients).

### CI Integration
Criterion benchmarks run on every PR. If a code change causes a statistically significant regression (e.g., > 5% slowdown in geodesic batch throughput), the CI pipeline flags the PR for review before merge. Prevents silent performance degradation as the codebase evolves.
