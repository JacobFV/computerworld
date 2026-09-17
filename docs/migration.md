# Provenance and migration

The detailed [provenance matrix](provenance.md) and
[pinned source revisions](../research/sources.json) describe predecessor evidence.
This workspace is a Rust redesign; it is not a source-compatible rename of any
prior project and does not import predecessor runtime dependencies.

| Predecessor | Retained idea or behavior | Migration consequence |
|---|---|---|
| SCE | Generic topology, independent machines, VFS/process/network semantics, OS profiles and service placement | Convert topology to `WorldDefinition`; register native implementations |
| TCN vendored SCE | Deterministic IDs/time, persistent sessions, clock reset fixes, actor/evaluator split | Keep one runtime alive; replace wrapper globals and model-specific projections with explicit session grants |
| SynthUX | Stateful service workflows, logical scheduling and input/frame provenance | Move durable state into service instances; route views through actual HTTP responses |
| Standalone mock sites | Independent endpoints and useful mail/docs/chat/git/calendar/issue views | Reexpress pages in the native page/scene contract; do not retain disconnected fixture stores |
| Synthex | Early virtual-internet intent and later SynthUX integration | No Internet-stub implementation to preserve |
| symbolic-ai-models | Explicit perception/action boundaries | Keep interfaces, omit model/training architecture |

Intentional incompatibilities include no mandatory Node/Python host runtime, no
Chromium-per-world rendering, no global `Date` patch, no subprocess-per-step bridge,
no implicit host network fallback and no hardcoded reference ecosystem. Reward
functions, training curricula and policy representations stay outside the kernel.

Portable checkpoints are new runtime artifacts, not imports of predecessor heap
snapshots. Commands implement documented synthetic subsets rather than a complete
POSIX/PowerShell interpreter. Native pages are not arbitrary HTML. Extension
handlers are trusted Rust code; former JavaScript/Python handlers require ports or
an explicitly separate adapter.

Use integration behavior rather than route-name similarity to assess parity:
two-machine mutations, DNS failures, authorization, replay, fork isolation and
actual visible-state transitions. Existing predecessor defects are evidence to
avoid, not compatibility requirements. Performance comparisons must use actual
successful work rather than requested-action echoes or unconditional success flags.
