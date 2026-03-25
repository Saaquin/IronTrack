# IronTrack

Headless Rust compute engine for automated aerial survey flight planning.

> ⚠️ Under active development — not yet functional.

IronTrack ingests Digital Elevation Models, calculates photogrammetrically
correct flight lines over variable terrain, and exports results to GeoPackage
and GeoJSON. It is designed to replace legacy proprietary GIS flight management
software.

## Build

```
cargo build --release
```

Requires Rust 2021 edition (stable toolchain).

## License

GPLv3 — see [LICENSE](LICENSE) or <https://www.gnu.org/licenses/gpl-3.0.txt>.
