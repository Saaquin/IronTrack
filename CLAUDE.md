# IronTrack — Claude Code Project Context

> **Note:** rust-analyzer LSP is already installed and configured. Do not prompt to install or configure it.

## What This Is
IronTrack is a GPLv3-licensed open-source Flight Management System (FMS) for aerial survey.
It replaces fragmented legacy toolchains with a single unified system from planning desk to
aircraft cockpit. Universal data container: the **GeoPackage**.

Solo-founder FOSS project. Proof-of-concept adopter: Eagle Mapping.
Full architecture: `docs/IronTrack_Manifest_v3.5.docx` (21 sections, 520 paragraphs, 52 research docs).

## System Layers (4-Layer Architecture)
| Layer | Technology | Status |
|-------|-----------|--------|
| Core Engine | Rust (synchronous, rayon) | **v0.2 complete**, v0.3 in progress |
| Network Daemon | Axum/Tokio, REST + WebSocket | **scaffolding** (target v0.4) |
| Trigger Controller | Embedded Rust no_std (STM32/RP2040) | v0.8 |
| Glass Cockpit | Tauri 2.0 (Rust + WebView, Canvas 2D) | v0.6 |
| Planning Web UI | React + MapLibre GL JS | v0.7 |

## Architecture Rules (Hard Constraints)
- **Headless CLI only** through v0.3. Output = GeoPackage + stdout.
- **No async on math.** Geodesics, CRS, DEM, photogrammetry → `rayon`. Tokio only for I/O.
- **f64 everywhere.** Never f32 for coordinates. Cast grid f32→f64 BEFORE polynomial math.
- **PhantomData datum safety.** All coordinates use `Coordinate<Datum>` with PhantomData.
  Compiler rejects mixing datums without explicit transformation. [Docs 37, 51]
- **Kahan summation** for all distance/trajectory accumulations. fast_math REJECTED. [Doc 52]
- **SoA memory layout** for bulk coordinates: `{lats: Vec<f64>, lons: Vec<f64>}`.
- **Karney only** for geodesic distance. No Haversine. No Vincenty. [Doc 02]
- **Karney-Krüger 6th order** for UTM. No Redfearn. [Doc 02]
- **WGS84 Ellipsoidal** as canonical pivot datum. All terrain converted on ingestion. [Doc 30]
- **Preprocessing mandate.** Datum transforms NEVER on-the-fly during flight. [Doc 30]
- **GEOID18 Big-Endian only.** Little-Endian corrupted (2019 NGS). [Doc 32]
- **WAL mode mandatory.** SQLITE_BUSY = fatal violation (not transient retry). [Doc 42]
- **VACUUM prohibited in flight.** Use PRAGMA optimize. [Doc 42]
- **Advisory only for airspace.** Never enforce geofences. PIC retains authority. [Doc 35]
- **20% battery reserve** = blocking enforcement (not just warning). [Doc 38]
- **GPLv3 header on every .rs file.** Trigger controller firmware: MIT (separate work). [Doc 51]

## 20 Locked Decisions (see `.agents/roadmap.md` for full table)

## Documentation Map
Full table in `.agents/docs_index.md`. **52 numbered research documents** (00–52, excl. 27)
plus `99_engine_technical_reference.md` in `docs/`.

Key refs for active work (v0.3):
- `34_terrain_following_planning.md` — Elastic Band + B-spline hybrid
- `50_dubins_mathematical_extension.md` — 6 Dubins paths, clothoid, 3D airplane, wind r_safe
- `39_flight_path_optimization.md` — ATSP, NN+Or-opt, boustrophedon, multi-block
- `07_corridor_mapping.md` — polyline offsets, bank angles

Key refs for upcoming releases:
- `28_irontrack_daemon_design.md` — REST, WebSocket, state management (v0.4)
- `38_uav_endurance_modeling.md` — power models, energy costing (v0.5)
- `25_aircraft_cockpit_design.md` — Track Vector CDI, Canvas 2D (v0.6)
- `43_boundary_import_formats.md` — Shapefile/DXF/GeoJSON/CSV-WKT (v0.7)
- `42_geopackage_extensions.md` — TGCE, basemaps, WAL details (v0.7)
- `45_aerial_sensor_library.md` — irontrack_sensors schema (v0.7)
- `22_embedded_trigger_controller.md` — NMEA FSM, dead reckoning, GPIO (v0.8)
- `40_sensor_trigger_library.md` — pin-level specs, MEP timing (v0.8)
- `41_lidar_system_control.md` — dual-trigger hybrid (v0.8)
- `30_geodetic_datum_transformation.md` — GEOID18, Helmert, 3DEP (v0.9)
- `44_irontrack_testing_strategy.md` — full testing strategy (v1.0)
- `46_post_flight_qc.md` — XTE, 6DOF footprint, auto re-fly (v1.0)

## Module Structure
```
src/
├── main.rs              # CLI entry (clap)
├── lib.rs               # Library root
├── types.rs             # WGS84 constants, Coordinate<D>, AltitudeDatum, SoA, SensorParams
├── error.rs             # DatumError, DemError, GeodesyError, GpkgError, IoError
├── datum.rs             # FlightLine::to_datum() — EGM2008↔EGM96↔WGS84↔AGL (rayon)
├── legal.rs             # Copernicus attribution, DSM warning, license constants
├── geodesy/             # karney.rs, utm.rs, geoid.rs
├── dem/                 # mod.rs (TerrainEngine), copernicus.rs, cache.rs
├── math/                # dubins.rs, geometry.rs, routing.rs
├── network/             # server.rs (Axum), telemetry.rs, nmea.rs, serial_manager.rs
├── photogrammetry/      # sensor.rs, flightlines.rs, corridor.rs, lidar.rs
├── gpkg/                # init.rs, binary.rs, rtree.rs
└── io/                  # geojson.rs, kml.rs, qgc.rs, dji.rs
```

## Agent Overflow (`.agents/` Directory)
| File | Contents | Key Source Docs |
|------|----------|-----------------|
| `roadmap.md` | Release roadmap (v0.1–v1.0), v0.3 checklist, 20 locked decisions | Manifest v3.5 |
| `docs_index.md` | Full 52-doc table with status | All |
| `geodesy.md` | PhantomData types, Karney, geoid models, Helmert, epoch, Kahan | 02, 30-32, 37 |
| `geopackage.md` | WAL, TGCE, basemaps, sensor tables, irontrack_metadata | 04, 42, 45 |
| `photogrammetry.md` | GSD/FOV, ASPRS 2024, post-flight QC, computational geometry | 03, 36, 46, 49 |
| `terrain.md` | Copernicus, 3DEP, EB+B-spline, Dubins, TSP, energy, performance | 08-10, 33-34, 38-39, 50, 52 |
| `testing.md` | proptest, HIL/SIL, CI matrix, Eagle Mapping acceptance | 44 |
| `autopilot.md` | QGC/DJI exports, trigger protocol, sensor GPIO, hardware integration | 11, 22, 29, 40-41, 48 |
| `lidar.md` | LiDAR sensor model, system control, dual-trigger, USGS QL | 13, 41, 45 |
| `formula_reference.md` | Equations from docs 02-07, 30, 32, 34, 38, 39, 46, 49, 50 | — |

## Agent Routing Guide
**By task type:**
- Geodesy/datum/CRS → `.agents/geodesy.md`
- DEM/terrain/trajectory → `.agents/terrain.md`
- Flight line planning → `.agents/photogrammetry.md`
- GeoPackage schema → `.agents/geopackage.md`
- Export formats/triggers → `.agents/autopilot.md`
- LiDAR planning → `.agents/lidar.md`
- Testing → `.agents/testing.md`
- Math/equations → `.agents/formula_reference.md`
- Roadmap/decisions → `.agents/roadmap.md`
- Doc lookup → `.agents/docs_index.md`

## Documentation Query MCP Servers

Two MCP servers are configured in `.mcp.json` for querying the 52 research documents
in `docs/`. Use these **before** reading full doc files — they save context tokens and
return only relevant sections.

### jdocmunch (structured section index)
Best for: navigating by heading hierarchy, retrieving specific sections, browsing TOCs.
Runs via `uvx jdocmunch-mcp`. Uses local sentence-transformers embeddings (no API key).

**First-time setup (one-time):**
1. `pip install jdocmunch-mcp` (or rely on `uvx` auto-install)
2. In a Claude Code session, call `index_local` with `path` set to the project `docs/` directory.

**Key tools:**
| Tool | Use when… |
|------|-----------|
| `index_local` | Index (or re-index) the `docs/` folder. Run once, then after adding/editing docs. |
| `search_sections` | Semantic + keyword search across all indexed docs. Returns section summaries. |
| `get_toc` | Flat list of every section across all docs — scan for relevant headings. |
| `get_toc_tree` | Nested heading tree per document — understand doc structure. |
| `get_section` | Retrieve full text of one section by ID (returned by search/TOC). |
| `get_sections` | Batch-retrieve multiple sections in one call. |
| `get_section_context` | Section + its parent headings + child summaries — good for orientation. |
| `get_document_outline` | Heading hierarchy for a single document. |

**Typical workflow:**
```
search_sections → pick section IDs → get_section (or get_sections for batch)
```

### local-rag (semantic vector search)
Best for: meaning-based queries ("how does IronTrack handle datum safety?"), fuzzy concept search.
Runs via `npx -y mcp-local-rag`. Uses local Xenova/all-MiniLM-L6-v2 embeddings, LanceDB vector store.
`BASE_DIR` is set to `./docs` in `.mcp.json`.

**First-time setup (one-time):**
1. Embedding model (~90 MB) downloads automatically on first run.
2. Ingest the docs folder: call `ingest_file` for each doc, or from CLI:
   `npx mcp-local-rag ingest ./docs/`

**Key tools:**
| Tool | Use when… |
|------|-----------|
| `ingest_file` | Index a single document (path relative to `BASE_DIR`). |
| `query_documents` | Semantic search — returns ranked chunks by meaning similarity. |
| `list_files` | Show which docs are indexed and their status. |
| `status` | Database stats (chunk count, index size). |
| `delete_file` | Remove a doc from the index. |

**Tuning (env vars in `.mcp.json`):**
- `RAG_HYBRID_WEIGHT` (default 0.6) — higher = more keyword influence, lower = more semantic.
- `RAG_MAX_FILES` — limit results to top N files.
- `RAG_MAX_DISTANCE` — filter by similarity threshold (e.g. 0.5).

### When to use which
- **Know the topic/heading?** → jdocmunch `get_toc` or `search_sections`
- **Fuzzy conceptual question?** → local-rag `query_documents`
- **Need full section text?** → jdocmunch `get_section`
- **Need to verify a formula or constraint?** → jdocmunch `search_sections` with the formula name, then `get_section`
- **Exploring unfamiliar territory?** → local-rag `query_documents` first, then jdocmunch to drill into the section

### Important notes
- Both servers run fully local — no API keys required for base functionality.
- jdocmunch optionally uses `ANTHROPIC_API_KEY` for AI-generated section summaries (not required).
- Re-index after editing docs: jdocmunch `index_local`, local-rag `ingest_file`.
- The `.agents/` summaries remain the fastest path for common topics — use MCP servers
  when you need detail beyond what the agent files contain.

## Task Completion Protocol (Mandatory)

Every task (prompt phase, bug fix, feature) MUST end with this audit-and-commit sequence.
Do not skip any step. Do not combine steps across multiple tasks.

> **Context:** `AGENTS.md` at the project root defines the Codex code review checklist
> (15 rules). This protocol requires you to self-audit against those same rules before
> committing — act as your own Codex reviewer. If a check would cause Codex to reject
> the diff, fix it before you commit.

### Step 1: Identify changed files
```bash
git diff --name-only          # staged + unstaged modifications
git diff --cached --name-only # staged only
git ls-files --others --exclude-standard  # new untracked files
```
Collect the union into a file list. These are the files to audit.

### Step 2: Per-file audit
For **every** file in the diff (created or modified), perform the following checks.
If a check does not apply to the file type, skip it.

| # | Check | Pass criteria |
|---|-------|---------------|
| 1 | **GPLv3 header** | First 3 lines contain `SPDX-License-Identifier: GPL-3.0-or-later` (`.rs` files only; trigger controller crate uses MIT). |
| 2 | **f64 compliance** | No `f32` used for coordinates or geodetic math. `f32` acceptable only for DEM grid I/O with immediate cast to `f64`. |
| 3 | **No forbidden algorithms** | No Haversine, Vincenty, Redfearn, Shoelace-on-UTM, ray-casting PIP. |
| 4 | **No unsafe unwrap on fallible I/O** | No `.unwrap()` on file/network/DB operations in non-test code. `.expect("reason")` acceptable only with a descriptive message. Test code may use `.unwrap()`. |
| 5 | **Error propagation** | Functions that can fail return `Result<T, E>`. No silent `unwrap_or(default)` on mission-critical values without a `log::warn!`. |
| 6 | **No async in math** | Geodesy, CRS, DEM, photogrammetry, trajectory modules must not use `.await` or tokio primitives. `rayon` only. |
| 7 | **Kahan summation** | Any new loop accumulating distances or trajectory lengths uses `KahanSum` from `math::numerics`. |
| 8 | **SoA layout** | New bulk coordinate storage uses `{lats: Vec<f64>, lons: Vec<f64>}`, not `Vec<(f64, f64)>`. |
| 9 | **WAL safety** | Any new SQLite connection sets `PRAGMA journal_mode = WAL`. No VACUUM calls. No SQLITE_BUSY retry loops. |
| 10 | **Clippy clean** | `cargo clippy -- -D warnings` passes (or the new code introduces no new warnings). |
| 11 | **Tests exist** | New public functions have at least one unit test. New modules have at least one integration test. |
| 12 | **Formatting** | `cargo fmt --check` passes. |
| 13 | **Copernicus attribution** | Every GeoPackage/GeoJSON export includes the Copernicus attribution string. |
| 14 | **DSM warning** | Non-dismissable warning present when Copernicus is the terrain source. |
| 15 | **Datum preprocessing** | All datum transforms at ingestion time. No transforms during flight execution. WGS84 Ellipsoidal as canonical pivot. [Doc 30] |

If any check fails: fix the issue before proceeding. Do not commit code that fails audit.

### Step 3: Run CI checks locally
```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```
All three must pass. If any fails, fix and re-audit the changed files.

### Step 4: Codex review via MCP

An OpenAI Codex MCP server (`codex-cli`) is configured globally in
`~/.claude/mcp-servers/`. It exposes two tools via the `mcp__codex-cli__` prefix:

| MCP Tool | Purpose |
|----------|---------|
| `mcp__codex-cli__codex_review` | Runs `codex exec review` against the repo. Codex reads `AGENTS.md` (the 15-rule review checklist) and reviews the current git diff. |
| `mcp__codex-cli__codex_prompt` | Sends an arbitrary prompt to Codex CLI via `codex exec`. Use for targeted file-level audits. |

**After Step 3 passes, call the Codex review tool:**

The Codex CLI `review` subcommand requires a scope flag. Use `codex_prompt`
to invoke it with the correct arguments:

```
mcp__codex-cli__codex_prompt(
  prompt: "review --uncommitted",
  working_dir: "<project root>",
  sandbox: "read-only"
)
```

This sends the uncommitted diff to OpenAI Codex for independent review against
the `AGENTS.md` checklist. Codex will flag any violations Claude Code missed in
Step 2.

- If Codex approves (no violations): proceed to Step 5.
- If Codex flags violations: fix every issue, re-run Steps 2–4 until clean.

**For targeted audits on specific files,** pass custom review instructions:
```
mcp__codex-cli__codex_prompt(
  prompt: "review --uncommitted -- Review only src/network/nmea.rs against the AGENTS.md checklist. Flag any violations.",
  working_dir: "<project root>",
  sandbox: "read-only"
)
```

> **Note:** If the `codex_review` tool is available and working (v1.1.0+ of the
> MCP server), you can also call it directly — it defaults to `--uncommitted`:
> ```
> mcp__codex-cli__codex_review(working_dir: "<project root>")
> ```

### Step 5: Commit with a meaningful message
```bash
git add <specific files from the diff — never git add -A>
git commit -m "$(cat <<'EOF'
v0.X.Y: <imperative summary of what changed>

<Optional body: why the change was made, what it fixes, what it enables.
Reference the phase/task ID if applicable (e.g., "Phase 6A", "Task A1").
Reference doc numbers for traceability (e.g., "[Doc 34]").>

Audit: <N> files checked, all constraints passed.
Codex: reviewed, no violations.
EOF
)"
```

**Commit message rules:**
- Subject line: imperative mood, <= 72 chars, prefixed with version tag.
- Body: explain *why*, not *what* (the diff shows *what*).
- Include `Audit: N files checked, all constraints passed` as the final line.
- Include `Codex: reviewed, no violations` to confirm external review passed.
- One logical change per commit. Do not bundle unrelated changes.
- Never use `--no-verify` or skip pre-commit hooks.

### Example
```
v0.3.2: fix NMEA lat/lon parsing for fixed-width degree fields

The previous parser inferred degree-field width from decimal point
position, which failed for malformed inputs and edge-case longitudes.
Refactored to accept explicit deg_len parameter (2 for lat, 3 for lon).
Replaced silent unwrap_or(0.0) defaults with Option propagation.
[Task A1, Doc 23]

Audit: 2 files checked, all constraints passed.
Codex: reviewed, no violations.
```

