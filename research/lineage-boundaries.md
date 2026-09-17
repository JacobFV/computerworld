# Synthex and symbolic-ai-models archaeology

Inspected full Git histories and source at these immutable revisions (2026-09-17):

- `commandagi/synthex`: `6c8d624d72e8e0a19869fe8546668ce9e4b2f3a8` (1,207 reachable commits).
- `jacobfv/symbolic-ai-models`: `328a6025b0baf072f97ace439c23840510101487` (777 reachable commits).

Clones: `/tmp/computerworld-sources/{synthex,symbolic-ai-models}`. No AGENTS.md exists in either clone or the workspace/ancestor directories examined. This report does not implement simulator code.

## Provenance matrix

| subsystem | best current implementation in inspected scope | source repo/path | important alternatives | what should survive |
|---|---|---|---|---|
| Early virtual internet | No functioning implementation: five-line class with empty `dns`, `email_routes`, `websites` dictionaries | synthex `synthetic_worlds/domains/internet/__init__.py` | Later SynthUX and SCE implementations must be assessed elsewhere | Historical conceptual namespace only; no algorithms to port |
| Basic synthetic UI actions | Element role/name/bounds plus input events | synthex `synthetic_worlds/domains/ui/__init__.py` | Later live DOM backend | Separate semantic regions, hit targets and input events, not fake URL assignment as navigation |
| Actual computer-use compatibility | Live Playwright desktop adapter to SynthUX | synthex `synthetic_worlds/grounding/adapters/web_desktop.py` | Simple `SyntheticPage` fixture | Optional explicit real-browser adapter, independently packaged; no compulsory browser process |
| Causal trajectory explanation | Episode steps carry thought, precondition proofs, effects and virtual time | synthex `synthetic_worlds/ontology/{simulate,schema}.py` | Original node expansion/trace kernel | Versioned causal events with stable identities; proofs only as optional evaluator metadata |
| Observation projection | `GroundedWorld.snapshot()` projects state through grounders; actuators dispatch by relation | synthex `synthetic_worlds/grounding/{world,simulate}.py` | New typed actor view | State/projection/action separation; do not mistake a projection for a checkpoint |
| Agent interface | Stateful reset/step, observation-only act, move records | symbolic-ai-models `symbolic_ai_agents/envs/{base,rollout}.py` | Model/task harness, teacher-forced histories | Persistent sessions, actual consequences, explicit observation/action capabilities, optional feedback |
| Hidden-state boundary | Structurally masked GraphView plus oracle-poisoning tests | symbolic-ai-models `symbolic_ai_core/graphview.py`, `tests/{test_environments,test_agents}.py` | Same env object exposes oracle to agents (weak boundary) | Remove privileged values from actor payloads; separate evaluator capability; adversarial noninterference tests |
| Seed discipline | Per-episode RNG derived from environment name, instance ID and config salt; stable SHA-256 tie-breaks in synthex | symbolic-ai-models `symbolic_ai_agents/envs/base.py`; synthex `ontology/simulate.py` | Global mutable Python RNG in WorldRoot | Versioned deterministic named seed streams, no host hash/time/UUID |
| Snapshot/fork | No portable complete checkpoint in inspected scope | synthex `core/{resource,clone}.py` | Ontology immutable graph extensions | Borrow semantics only conceptually; implement typed serialization/COW afresh |

## Lineage: verified facts, not repository-order guesses

Synthex's initial commit `c1c36ca` (2026-05-31, “Implement synthetic worlds MVP kernel”) introduced the VirtualInternet stub. `git log --all -- synthetic_worlds/domains/internet` has exactly this one entry, and searching `synthetic_worlds` finds no references to its three dictionaries outside the stub. This is not DNS, mail delivery, routing, or HTTP. `NavigateToURL._execute()` merely assigns `page.url`; no response is fetched. `DesktopVM` acquires a synthetic resource dictionary, not a VM.

Synthex later **consumes SynthUX**, rather than simply being its ancestor: `1b125fb` (2026-06-01) added `grounding/adapters/web_desktop.py` driving existing SynthUX desktop URLs through Playwright; `2dc168d` added all three desktop variants. Its `_GETSTATE_JS` speaks `synthux.getState` over `postMessage`, and app launch selectors mirror SynthUX's native app selector tables. This is a bridge dependency, not proof that SynthUX copied Synthex's internet class. Later `916d940` adds a SynthUX loader for computer-use experiments, and `1ed95d0` adds a live closed-loop ladder. Treat the relationship as a graph with subsequent reuse in both directions where independently evidenced.

`de85b15` fixed a real-browser recording bug: close the Playwright page context before the browser so video finalizes; browser-only close had produced empty WebM output. Preserve this lifecycle detail if an optional browser adapter is shipped.

Symbolic-ai-models has a nontrivial migration loss: `9fc9ae8` (2026-08-13) explicitly recovered 29 `symbolic_ai_agents` modules and four tests from branch `omegaclaw`; the interactive package had remained off mainline while master advanced 94 commits. Current wheel `only-include` in `pyproject.toml` still omits `symbolic_ai_agents` even though its console-script entry exists. Thus source availability and wheel usability are different claims.

`262e312` (2026-08-10) moved parser primitives out of model-owned code to eliminate a dependency cycle. `symbolic_ai_models/parsers/primitives/symbols.py` records the deeper boundary lesson: supplying the agent with environment-built constructors can hand it the parse it is supposed to infer. Preserve independent serializable environment schemas; do not import policy/model representation types into the simulator. Structured observations remain valid when intentionally declared as an interface, not silently substituted for pixels/text.

## Actual determinism and snapshot behavior in Synthex

- `core/world.py` uses `random.Random(seed)` and a stable SHA-256 root ID derived from dataset name, split and seed. This is a useful explicit seed pattern, but dataset metadata should not own core world identity.
- `core/scheduler.py` deterministically traverses nodes in breadth-first order, ordering non-concurrent children by priority/path and concurrent children by path. It repeatedly materializes full traversal lists and uses `pop(0)`; it is a tree-expansion scheduler, not an efficient deterministic network/event scheduler. Port the stable ordering principle, not this algorithm.
- `core/event_bus.py` sorts events by time but records consumed **list indexes**. Reproduced using the source modules: emit event `later@10`, consume -> `['later']`; emit `earlier@5`, consume -> `['later']` again. Earlier insertion invalidates consumed indexes. Use immutable event IDs plus a monotonic insertion sequence and ordered scheduler keys instead.
- `core/clock.py` rejects negative relative advances but `at(timestamp)` can move backwards. New core should explicitly define monotonic step time versus restore/reset time.
- `core/resource.py` has genuine in-memory snapshot/restore via `deepcopy`, but resource handles use `uuid.uuid4()`, snapshot IDs hash `repr` of state, and payload type is unconstrained `Any`. Verified equivalent independently acquired resources receive different snapshot IDs. There is no portable serialized world checkpoint contract here.
- `core/clone.py` deep-copies candidate node trees, shares resource handles as borrowed leases, and falls back to the original object if deepcopy raises. This cannot promise isolated deterministic forks of arbitrary backends.
- `runners/replay.py` invokes the local runner again with the same example and seed; it does not consume a recorded action/event stream. Name it seeded reconstruction, not general trajectory replay.
- `grounding/world.py::snapshot()` returns an ontology projection of live backend observations plus symbolic overlay. It cannot restore a browser, files, physics state or clock. Keep projection separate from checkpoint APIs.
- `ontology/simulate.py` is a real forward causal graph simulator: it selects applicable affordances, proves preconditions, records effects, extends graph facts, advances synthetic duration, rejects contradictory functional relations, and uses SHA-256 seeded tie-breaking. `episode_from_plan` rechecks preconditions against evolving state. These are valuable causal-accounting ideas, but this planner/model ontology must not become a mandatory computer kernel.
- `core/trace.py` produces versioned records with seed/time/root/node path and JSONL export. It also opens an optional host file per record; this host sink belongs behind an adapter, and traces are not automatically replay-complete event sourcing.

## Boundary ideas to preserve and strengthen

`symbolic_ai_agents/envs/base.py` separates immutable Observation, Step, Move and Trajectory records and makes sessions stateful. `rollout.py` runs exactly the actions the agent chose: errors become the next observation rather than silently restoring a correct teacher-forced history. Actions versus information-buying probes are an interface-level distinction. A simulator should support these through configurable action families, not define question-answer commitment or rewards in the kernel.

`symbolic_ai_core/graphview.py::GraphView` genuinely removes hidden labels and types (`None`) and withholds hidden coreference relations. It does not keep a spare gold-label field in the exposed graph. Tests assert this. `tests/test_agents.py::test_no_agent_consults_the_oracle` changes oracle answers to lies and requires identical action/confidence trajectories. Reuse the test pattern for privileged inspector changes, hidden task objectives, inaccessible filesystem data and evaluator metadata.

However, agents receive the full Environment object (`agent.begin(env, obs)`), which includes the `oracle()` method. This is convention plus regression tests, not access control. `Observation.info` and `Step.info` are untyped escape hatches. New Rust actor handles should expose only configured actions/observation channels; evaluator inspection should require a separate handle/capability, never serialized into actor responses. Reward/success computation belongs in an optional task wrapper consuming evaluator access.

`episode_rng()` uses stable string seeds instead of Python's salted `hash()` and incorporates config changes. Carry this intent into specified RNG algorithms and stable canonical config encoding; do not promise Python/Rust stream compatibility accidentally. For many world components, derive independent streams by stable component IDs so unrelated initialization additions do not perturb existing components.

`ed01df4` fixed baseline fitting asymmetry: the reference policy had different training environments/seed than the scored agent, yielding spurious positive lift. This is an evaluator lesson, not simulator machinery. Retain matched-world action trace comparability and explicit run metadata; omit baseline policies, calibration, tokenizers, model fitting, symbolic parsers, curricula, proofs-as-policy and all Torch/NumPy dependencies from the simulator.

## Licensing and validation scope

Synthex declares MIT in `pyproject.toml`; no top-level LICENSE file was found. Symbolic-ai-models has no top-level LICENSE or project license declaration; `symbolic_ai_data/LICENSE` is MIT for that vendored subtree, and KG datasets have their own licenses. Do not assume that subtree license licenses the agent/core files. Prefer architectural ideas and clean Rust re-expression; resolve file-level rights before directly copying unlicensed source.

Read code and Git history; ran targeted dependency-free Python reproductions for EventBus and ResourceManager behavior. Did not run full predecessor model/browser test suites: they involve substantial unrelated dependencies and external desktops, and no benchmark numbers are claimed here. Concrete behavior established by these probes should become regression cases in the new project's tests.
