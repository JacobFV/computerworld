# Workspace map

Package names are stable; the directories group related implementations. Use
`cargo test -p <package>` to work on one package without depending on its location.
The [runtime architecture](../docs/architecture.md) explains how they fit together.

| Directory | Responsibility | Packages |
| --- | --- | --- |
| [core/](core/) | Contracts, scheduling, actor sessions, recording and world construction | `cw-protocol`, `cw-determinism`, `cw-sdk`, `cw-kernel`, `cw-environment`, `cw-trajectory`, `cw-evaluation`, `cw-blueprint` |
| [machines/](machines/) | Filesystems, processes, shell, network and explicit host adapters | `cw-computer`, `cw-network`, `cw-host-adapters` |
| [languages/](languages/) | Embedded interpreters and deterministic runtime utilities | `cw-pyvm`, `cw-jsvm`, `cw-script-host`, `cw-regex`, `cw-tz`, `cw-zlib` |
| [web/](web/) | HTML/CSS/DOM engine and browser shell | `cw-web` in `engine/`, `cw-browser` in `browser/` |
| [graphics/](graphics/) | Scene descriptions, rendering, artwork and maps | `cw-scene`, `cw-render`, `cw-artwork`, `cw-map` |
| [engines/](engines/) | Application domain engines | `cw-cad`, `cw-eda`, `cw-sheet`, `cw-sql`, `cw-raster`, `cw-video` |
| [applications/](applications/) | Native applications and desktop presentation | `cw-applications` |
| [services/](services/) | Synthetic internet services and their shared runtime | `cw-services`, `cw-service-*` |
| [internet/](internet/) | The built-in internet every world joins unless it opts out | `cw-internet` |
| [bindings/](bindings/) | Thin Python and WebAssembly interfaces | `cw-python`, `cw-wasm` |
| [computerworld/](computerworld/) | Public Rust facade and end-to-end tests | `computerworld` |

Tests and fixtures stay with their owning package. Shared language examples are
in [examples/](../examples/README.md), input worlds in [worlds/](../worlds/README.md),
and benchmark runners in [benchmarks/](../benchmarks/README.md).

Choose an existing package and module when the responsibility fits. Add a crate
for a meaningful dependency/API boundary, not merely to shorten a file. Group
directories contain no manifests of their own. The root workspace lists all
package levels explicitly, and `scripts/checks/check-boundaries.py` recursively
checks simulation packages while allowing only the named host/binding edges.
