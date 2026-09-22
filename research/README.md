# Research and evidence

This directory preserves design research and captured evidence. Current runtime
behavior is documented in [the guides](../docs/README.md); older observations here
may describe earlier versions.

- [sources.json](sources.json): predecessor repositories and their source identities.
- [notes/](notes/): engine gaps, lineage boundaries and predecessor studies.
- [studies/desktop-fidelity/](studies/desktop-fidelity/): desktop visual references and the original overhaul contract.
- [studies/gallery/](studies/gallery/): gallery experiments, captures and reproduction scripts.
- [studies/google-ceiling/](studies/google-ceiling/README.md): browser comparison, fixtures, scripts and results.
- [studies/site-stills/](studies/site-stills/): retained site-rendering reference captures.

Keep the script, inputs and selected evidence for a study together. Preserve
committed reference images when they support a recorded result; write exploratory
runs to ignored `target/research/<study>/` unless intentionally promoting a result
to a reference. Benchmark runners and their published baselines live separately in
[benchmarks/](../benchmarks/README.md).
