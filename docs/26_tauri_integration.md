# Tauri 2.0 Integration Architecture for the IronTrack Flight Management System

> **Source document:** 26_Tauri-Integration-IronTrack.docx
>
> **Scope:** Complete extraction — all content is unique to the project. Covers Rust engine mapping, IPC data marshalling, multi-window orchestration, daemon embedding, offline GeoPackage access, custom URI tile serving, cross-platform WebView differences, ARM cross-compilation, and auto-update safety governance.

---

## 1. Core Engine Integration

### 1.1 The Async Boundary in Tauri Context

Tauri relies internally on a tokio async runtime for its event loop, plugin initialization, and IPC message passing. IronTrack's math engine (geodesy, photogrammetry, DEM processing, datum conversions) runs synchronously on OS threads via rayon.

**The conflict:** If a Tauri command directly invokes a heavy synchronous calculation (e.g., generating terrain-following flight lines across thousands of DEM microtiles), it blocks the tokio worker thread. This starves the async runtime — halting NMEA telemetry ingestion and severing the WebSocket connection to the glass cockpit.

**The solution:** Every Tauri command that interfaces with the math engine must wrap the synchronous call in `tokio::task::spawn_blocking`:

```rust
#[tauri::command]
async fn generate_flight_plan(state: State<'_, AppState>, params: PlanParams) -> Result<PlanSummary, String> {
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || {
        engine.generate(params)  // synchronous rayon work
    })
    .await
    .map_err(|e| e.to_string())?
}
```

The tokio worker thread is immediately freed. The Tauri command awaits the result asynchronously and returns structured data to the frontend via IPC.

### 1.2 Inter-Process Communication (IPC) Mechanisms

| IPC Mechanism | Characteristics | IronTrack Use Case |
|---|---|---|
| **Commands** | Async, promise-based invocations. Args/returns must implement `serde::Serialize`/`Deserialize`. | Discrete user actions: generate flight plan, save sensor profile, query geodetic distance |
| **Events** | Fire-and-forget, bidirectional broadcast. Global or targeted to specific webview labels. JSON payloads. | Continuous streaming: live NMEA GPS coordinates, cross-track error updates, telemetry broadcasts to glass cockpit at 10 Hz |
| **Channels / Responses** | High-performance ordered streams supporting raw binary buffers (`Uint8Array`). No JSON array serialization. | Massive binary transfers: raw DEM microtiles, compiled GeoPackage blobs, SoA coordinate arrays |

**Binary transfer for SoA data:** Standard JSON serialization converts binary data to integer arrays — catastrophically large for coordinate datasets. IronTrack must use `tauri::ipc::Response` and `tauri::ipc::Channel` to pass raw memory buffers directly to the JavaScript `ArrayBuffer` interface with near-zero serialization overhead.

**Event-driven telemetry:** The Rust backend uses `app.emit()` to broadcast state changes at 10–20 Hz. The React frontend subscribes via `@tauri-apps/api/event`. This avoids the immense promise allocation overhead of polling the backend at high frequency.

### 1.3 Mutex Safety Across Async Boundaries

Shared mission state is accessed concurrently by HTTP requests, WebSocket connections, and Tauri IPC commands. Synchronization is required.

**Hazard:** `std::sync::Mutex` held across an `.await` point causes deadlock panics — the tokio executor cannot preempt the holding task.

**Options:**
- Use `tokio::sync::Mutex` for state accessed within async command handlers.
- Or structure synchronous critical sections so that `std::sync::MutexGuard` is explicitly dropped before any `.await` yield point.

---

## 2. Multi-Window Orchestration

### 2.1 Dual-Role Architecture

A single Tauri binary simultaneously serves the planning interface and the glass cockpit. Windows are instantiated via `tauri::WebviewWindowBuilder` with unique labels (e.g., `planning_view` and `cockpit_view`), pointing to dedicated HTML entry points or distinct routes within a unified SPA.

Field scenario: operator laptop connected to a secondary monitor — planning UI on primary screen, glass cockpit on secondary screen.

### 2.2 State Synchronization: Revision-Ordered Invalidation-Pull

Each webview operates in a strictly isolated JavaScript context — no shared memory or DOM between planning and cockpit windows.

**Pattern:**

1. The Rust backend is the **single source of truth**, maintaining master mission state + a monotonically increasing revision counter.
2. When a mutation occurs (e.g., operator changes flight line altitude in the planning window), the command updates the Rust backend.
3. The backend broadcasts a lightweight **invalidation event** containing only the new revision number to all active webviews.
4. Each React instance compares the incoming revision against its local state.
5. If stale, the window independently requests a fresh data snapshot from the backend via a subsequent command.

**Benefits:** All windows stay synchronized without spamming the IPC bridge with massive redundant data payloads during rapid user interactions.

---

## 3. Embedding the Axum Daemon

### 3.1 Three Deployment Topologies

**Option A — Embedded (standalone laptop):** Tauri spawns the Axum daemon in a background thread at startup. The laptop is simultaneously the FMS compute engine, network host, and display. Zero external configuration.

**Option B — Distributed (thin client):** A dedicated edge device (NUC, Pi) runs the daemon headlessly. The Tauri app on the pilot's tablet or operator's laptop connects as a thin client via WebSocket/REST. No local daemon initialization.

**Option C — Auto-detecting hybrid (default):** On startup, Tauri probes the network:

1. Attempt HTTP handshake on `127.0.0.1:3000` (or a cached network IP).
2. **Success:** External daemon detected → adopt thin-client mode (Option B).
3. **Connection refused:** No active daemon → spawn the daemon internally (Option A).

This eliminates configuration friction in high-stress aviation environments.

### 3.2 Threading: Isolating the Daemon Runtime

**Problem:** Both Tauri and Axum depend on tokio. Spawning Axum inside a Tauri command or setup hook causes a fatal panic: "Cannot start a runtime from within a runtime."

**Problem compounded:** Using `tauri-plugin-axum` couples the HTTP server to the webview's execution thread. If the pilot minimizes the cockpit window or the canvas re-render hangs, the trigger controller stops receiving waypoints.

**Solution:** Spawn the daemon in a **completely independent OS thread** with its own tokio runtime:

```rust
tauri::Builder::default()
    .setup(|app| {
        let state = app.state::<SharedFmsState>().inner().clone();

        // Escape Tauri's async context entirely
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build daemon runtime");

            rt.block_on(async {
                let app = axum::Router::new()
                    .route("/api/v1/mission/plan", get(get_plan))
                    .route("/api/v1/telemetry/stream", get(ws_telemetry))
                    .with_state(state);

                let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
                    .await.unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });

        Ok(())
    })
```

The daemon's intensive network I/O (REST payloads, WebSocket multiplexing, NMEA serial) consumes its own dedicated thread pool, completely insulating the Tauri IPC bridge and GUI rendering pipeline.

---

## 4. Offline Capabilities

### 4.1 GeoPackage Access: Rust Backend Only

The Tauri frontend must remain **completely ignorant of filesystem mechanics**. All GeoPackage interactions go through the Rust backend via `rusqlite` with the `bundled` feature (statically compiled SQLite, zero system dependencies).

**Flow:** Frontend sends an IPC command with a file path → Rust backend opens the SQLite connection → executes spatial queries → performs datum conversions → serializes verified flight line arrays back to the frontend.

The Rust engine is the sole component trusted with life-critical aviation math. The UI cannot directly read or modify the GeoPackage.

### 4.2 Custom URI Protocol: Embedded Offline Tile Server

Serving thousands of DEM tiles or basemap images through standard IPC commands (JSON serialization) would throttle the application and destroy the frame rate.

**Solution:** Register a custom Tauri protocol interceptor `irontrack://`:

1. Frontend mapping library requests: `irontrack://tiles/dem/14/2345/3456.png`
2. Tauri intercepts the request at the OS level **before it hits the network stack**.
3. Rust backend receives the URL parameters, executes a spatial query against the local GeoPackage, extracts the raw binary tile blob.
4. Backend constructs a standard HTTP response with correct MIME type headers.
5. The frontend receives the tile as if it came from a remote server.

**Result:** The Tauri Rust backend functions as a **zero-latency, offline tile server**. MapLibre/Leaflet interact with `irontrack://` URLs identically to a cloud tile server. No IPC serialization overhead. No CORS restrictions. The glass cockpit maintains 60 FPS while processing gigabytes of local terrain data.

### 4.3 DEM Pre-Ingestion for Offline Flight

During the office planning phase, the engine downloads DEM tiles (Copernicus GLO-30 or 3DEP) for the survey area and caches them as binary microtiles inside the GeoPackage. The GeoPackage then carries its own terrain data to the field — no internet access required at altitude.

---

## 5. Cross-Platform Deployment

### 5.1 WebView Engine Differences

| OS | Engine | Characteristics | Cockpit Rendering Implications |
|---|---|---|---|
| **Windows 10/11** | WebView2 (Chromium/V8) | Excellent JS performance, modern web standards | Severe FPS degradation with stacked high-resolution Canvas layers. Minimize overlapping transparent canvases. Use WebGL for hardware acceleration. |
| **macOS** | WKWebView (WebKit/JSCore) | Aggressive JIT compilation, fluid 60 FPS for SVG/CSS 3D | Ideal for the operator planning interface. Excellent Canvas and SVG performance. |
| **Linux (Debian/Ubuntu)** | WebKitGTK (WebKit) | Historically CPU-bound. Modern 2.48+ versions add GPU Skia rendering and damage tracking. | Flatten the cockpit display into a single WebGL context to bypass DOM overhead. Critical for Pi/ARM performance. |

**Universal strategy:** Use Canvas 2D / WebGL for the cockpit display on all platforms. Avoid massive SVG DOM manipulation for real-time telemetry — it's unsustainable on WebKitGTK/ARM.

### 5.2 ARM Cross-Compilation for Raspberry Pi

Tauri apps must link against `glibc`, `webkit2gtk`, `libssl`, and `gtk3`. Cross-compiling from x86 to aarch64 requires matching these C dependencies for the target architecture.

**Critical constraint:** Build machine glibc version must be **≤** target device glibc version, or the binary crashes with "version 'GLIBC_X.XX' not found."

| Method | Approach | Suitability |
|---|---|---|
| Manual toolchain | Install `aarch64-linux-gnu-gcc`, inject multi-arch packages | Brittle, pollutes host system. **Not recommended.** |
| **Dockerized builds** | Container (e.g., `ghcr.io/tauri-apps/tauri/aarch64`) with QEMU emulation. Isolated `PKG_CONFIG_SYSROOT_DIR`. | **Production standard.** Repeatable, CI/CD-ready, guarantees target-matching glibc/WebKitGTK. |
| Cargo-zigbuild | Zig compiler (`zig cc`) as cross-linker. Auto-fetches correct glibc headers for target. | Emerging, excellent for rapid local iteration. |

**IronTrack CI/CD:** Dockerized builds in GitHub Actions, producing `.deb` packages matching the Pi's exact OS release.

---

## 6. Auto-Update and Avionics Safety Governance

### 6.1 The Mid-Campaign Update Hazard

Consumer apps auto-update silently. For an FMS processing life-critical aviation data, this is catastrophic:

- A mid-campaign update could alter geodetic datum conversion logic.
- A rendering bug could obscure the cross-track error indicator.
- A schema change could corrupt the active mission GeoPackage.
- The pilot would take off with a different software version than the one used during planning.

**In aviation, deterministic software behavior is paramount.**

### 6.2 Update Governance Architecture

**Disable Tauri's default auto-update polling** in `tauri.conf.json`. Suppress the default update dialog. Transfer total update control to the React frontend.

**Gating conditions** — the update `check()` function may only be invoked when:

1. Aircraft is definitively grounded.
2. Trigger controller is electronically disarmed.
3. No active mission state is loaded in memory.

**Update prompt must display:**
- Full release notes.
- Specific callouts for any modifications to geodetic algorithms.
- GeoPackage schema version changes (old plans may need re-export).
- Ed25519 cryptographic signature verification status (retained from Tauri's built-in system).

**Schema migration:** If the new version introduces schema changes, the update routine must either cleanly migrate existing GeoPackage files or explicitly warn the operator that older plans require re-exporting from the office engine.
