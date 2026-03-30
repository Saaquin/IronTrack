# **IronTrack Project Charter**

## **Executive Summary & Scope**

**Project Name:** IronTrack  
**Primary Objective:** To develop a memory-safe, highly concurrent open-source flight management and aerial survey planning alternative to legacy proprietary software, built on Rust.  
**Design Language:** Dark UI incorporating blacks and earth tones, themed around industrial/steam-engine aesthetics.  
**Minimum Viable Product (MVP) Scope:** The v0.1 release will focus strictly on the foundational GIS planning phase, specifically: \* Ingestion of DEM data (SRTM/3DEP) for accurate terrain modeling. \* Generation and calculation of flight line datasets based on sensor parameters and terrain. \* Data persistence and export utilizing a lightweight, database-backed format (GeoPackage).  
**Target Adopter & Proof of Concept:** The primary stakeholder and testing ground for IronTrack is Eagle Mapping. The software must successfully integrate into their existing flight planning and sensor operation workflows. Securing adoption within this production environment is the gating requirement before seeking wider community expansion.

## **Licensing and IP Governance**

**Open Source License:** GNU General Public License v3 (GPLv3). This strong copyleft license ensures IronTrack remains free and open-source in perpetuity. Any future modifications, distributions, or derivative works based on the IronTrack codebase must also be distributed under the same open terms, strictly preventing proprietary capture or vendor lock-in.  
**Project Governance & Intellectual Property:** IronTrack is an independent, founder-led FOSS initiative. All intellectual property, copyright, and initial codebase authorship are retained by the founder.  
**Corporate Relationship Boundary:** While the software is explicitly designed to solve the operational friction of commercial aerial survey providers (serving Eagle Mapping as the initial proof-of-concept environment), IronTrack is developed independently. There is no corporate sponsorship, ownership, or exclusive rights granted to any employer or commercial entity.

## **Architecture and Technology Stack**

**Core Engine Design:** IronTrack utilizes a decoupled architecture. The v0.1 MVP consists of a headless Rust compute engine, strictly separating the flight planning logic and geospatial processing from the graphical presentation layer.  
**Future-Proof Extensibility:** This decoupled approach ensures the Rust core can be easily wrapped by diverse frontends in later releases (e.g., a React-based web UI or a Tauri desktop application), allowing the project to scale visually without rewriting the underlying math.  
**Geospatial Processing & CRS:** The engine will natively handle Coordinate Reference System (CRS) transformations. While global data (like SRTM) will be ingested in WGS84, the engine will dynamically project working areas into appropriate localized CRS (e.g., UTM zones) to ensure geometrically accurate distance and area calculations for photogrammetry.  
**Data Interoperability:** The primary data store and exchange format is GeoPackage. The Rust engine interacts with the GeoPackage for persistence, lightweight database operations, and spatial data interplay, ensuring immediate compatibility with existing enterprise GIS software.

## **v0.1 Development Roadmap & Milestones**

**Milestone 1: Dynamic Terrain Ingestion and Caching:** Establish foundational terrain awareness. This involves building a microtile structure to efficiently handle large DEM datasets. The engine will download necessary large files, parse them, and utilize standard cross-platform caching (e.g., via Rust\&apos;s dirs crate) rather than OS-specific paths. The core output is the automatic calculation of optimal flightline altitudes based on underlying elevation data.  
**Milestone 2: Geospatial Sensor Geometry and Swath Logic:** Implement the core photogrammetric math layer. This focuses on calculating swath width and ground sample distance at a centimeter-level resolution across the footprint. It introduces user-adjustable parameters for sensor Field of View (FOV) and lateral overlap. A smart overlap-checking algorithm will validate adjacent flight lines and ensure gapless coverage over variable terrain.  
**Milestone 3: Core I/O and Reporting:** Finalize the v0.1 MVP by building robust primary I/O capabilities. To maintain scope, export targets for v0.1 are strictly limited to GeoPackage (as the rich data store) and GeoJSON (as a lightweight, web-friendly plaintext alternative). Automated flight reporting will be implemented, outputting operational metadata embedded directly within the GeoPackage structure.

## **Community Infrastructure & Engagement**

**FOSS Fundamentals:** For v0.1, establish baseline open-source infrastructure to signal a serious project. This includes a comprehensive README (with build instructions), a CONTRIBUTING.md guide, issue templates for bugs/features, and basic Continuous Integration (CI) via GitHub Actions to ensure memory-safe builds.  
**Workflow Agnosticism:** Explicitly document the operational workflows specific to Eagle Mapping versus those that are generalizable to the wider aerial survey industry. This prevents the core engine from quietly becoming bespoke internal software and ensures it remains valuable to a broad community.

## **Long-Term Sustainability & Funding**

**Modular Expansion:** Maintain the strict decoupling of the core engine. Future enterprise adoption (and potential foundation funding) will rely on the ability to swap in custom frontends, proprietary sensor modules, or advanced planning UI wrappers without touching the Rust core.  
**Community Maintenance:** As the project matures past v0.1, the goal is to transition from a solo founder model to a meritocratic technical oversight committee, encouraging commercial survey outfits to dedicate engineering time upstream to IronTrack to maintain long-term support (LTS) for industry-standard capabilities.  
