# computerworld

Proposed standalone synthetic computer-use environment: one deterministic Rust
runtime, many worlds, agent interfaces and language bindings.

**Status: investigation complete; architecture awaiting approval.** This repository
currently contains research and a design proposal. No simulator or installable
package has been implemented yet.

Read in order:

1. [Provenance matrix and lineage](docs/provenance.md)
2. [Architecture options, recommended design and proposed APIs](docs/architecture-proposal.md)
3. [Parallel implementation plan, tests and benchmarks](docs/implementation-plan.md)
4. [Checks performed during investigation](research/verification.md)

The proposal uses a pure Rust scheduler with typed subsystem state, explicit host
adapters, complete checkpoints, independent service instances and a custom scene
renderer. Native, Wasm/JavaScript and PyO3/Python use the same semantics. Actor
observations are distinct from privileged inspection and optional task evaluation.

Detailed source reports:

- [Unified synthetic-computer-environment](research/unified.md)
- [TCN vendored engine and downstream fixes](research/tcn.md)
- [SynthUX world, services and rendering](research/synthux.md)
- [Standalone mock services](research/services.md)
- [Synthex and symbolic-ai-models boundaries](research/lineage-boundaries.md)
- [Pinned source revisions](research/sources.json)

Predecessor checkouts live outside this repository. Their implementations have
not been concatenated or vendored into it. Publication and package names are not
yet configured.
