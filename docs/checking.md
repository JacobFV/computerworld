# Bounded model checking for agent safety

`scripts/checks/cw-check.mjs` enumerates every state a GUI agent can reach within `k`
actions of a seeded start state and evaluates a set of policies on each one. It
answers a question sampling cannot: not "did 500 rollouts misbehave" but "is
there *any* sequence of at most `k` clicks that reaches a state where this
policy is false".

The output is one of two things per policy. A certificate — the bound was
exhausted and no reachable state violates it. Or a counterexample — a literal
action sequence, which the tool then replays from a fresh world and re-checks
before it will write it down.

```sh
node scripts/checks/cw-check.mjs --depth 6 --out site/data/policies.json
node scripts/checks/cw-check.mjs --help
node scripts/checks/cw-check.mjs --self-test      # the evaluator's unit tests
```

| Option | Meaning |
|---|---|
| `--world <path>` | World definition. Default `worlds/company-2026/world.json`. |
| `--seed <n>` | World seed. Default `7`. |
| `--depth <k>` | Maximum action depth. Default `6`. |
| `--budget <n>` | Maximum states to expand before stopping. Default `20000`. |
| `--time-budget <s>` | Stop expanding after this many seconds. Default `5400`. |
| `--viewport <WxH>` | Virtual display. Default `1440x2000`. |
| `--pkg <dir>` | The Wasm Node package. Default `pkg/node`, built by `scripts/build-wasm.sh`. |
| `--out <path>` | Where the report goes. Default `site/data/policies.json`. |
| `--progress <n>` | Print a line every `n` expanded states. |
| `--exact-state` | Read world state only through `inspect()`, never through the snapshot shortcut below. |
| `--cross-check <n>` | Check the snapshot read against `inspect()` every `n` states. Default `250`. |
| `--no-verify` | Skip counterexample replay. Not recommended; the replay is the honesty check. |

The report is rewritten at every depth boundary. A run stopped part way through
therefore still leaves a complete certificate for the last depth it finished,
and the report says how much of the next depth it had got through.

Progress, the per-policy verdicts and the termination reason all go to stdout.
A run that stops on its budget says so, in the report as well as on the terminal.

## What is checked

The default policy set, `buildPolicies()` in the script, is fifteen rules about
the pull-request review-and-merge flow of `http://github.com/northstar/atlas` in
the `company-2026` world — the kind of merge policy a company writes down and
then expects a tool to respect. The flow was chosen because it has real business
logic behind it: a review has a decision, a merge moves the base ref, a draft is
not supposed to be mergeable, and a repository has a writer list.

The agent is `alice`, who is on that repository's writer list, driving a browser
on `alice-mac`. Nothing about the policies is specific to the checker: they are
ordinary `Predicate` trees with the temporal operators below, and the tool takes
whatever set it is given.

## What it drives

The checker uses four things the runtime already exposes, and nothing else.

| Call | Role in the search |
|---|---|
| `world.snapshot()` / `world.restore()` | Backtracking. A snapshot is a copy-on-write handle; restoring one puts the single world back on a frontier node. |
| `environment.scene(w, h)` | The successor generator. |
| `world.inspect()` | What the policies are evaluated against. |
| `world.exportSnapshot()` | The same state as one JSON string, for the fast read below. |
| `world.stateHash()` | Pins a counterexample's end state exactly. Not the visited-set key — see below. |

**The scene is the successor generator.** A state's enabled actions are the live
controls on screen — around thirty-five of them on these pages — not 10⁶ pixels. `scene(w, h)`
returns every node with its role, label, value, disabled flag, bounds, transform,
clip and z, before anything is rasterized, so the whole set falls out of one walk
over it. The tool keeps the nodes inside the browser window whose role is a real
control (`button`, `link`, `textbox`, `checkbox`, …), groups them by interaction
id, and finds a point a click actually reaches each one at by sampling its
transformed rectangle and taking the topmost interactive node covering the point.
Each control becomes one `pointer.v1 click`; each text field becomes one edge per
value in its abstract domain, a click followed by a `keyboard.v1 type`.

**Reading state is the expensive part.** `inspect()` encodes the whole world —
the definition, ninety-three services, five filesystems — as JavaScript objects,
and costs about 200 ms. The search needs one repository out of that, tens of
thousands of times. `exportSnapshot()` hands back the same state as one JSON
string in about 70 ms, and the repository can be cut out of that string and
parsed in well under a millisecond.

That shortcut is held to the exact path. The anchor `"github":{` occurs exactly
once in the export; if it ever does not, or the object cut out does not have the
shape of this repository, the read falls back to `inspect()` and the report
counts it in `state_read_fallbacks`. Before the search starts, the two reads are
compared at the start state and the run aborts if they differ. Every
`--cross-check` states they are compared again and the run aborts if they ever
disagree. Counterexample replay uses `inspect()` alone. `--exact-state` turns the
shortcut off entirely, which is about 2.3× slower and produces the same answers.

## The constraint language

`crates/evaluation/src/lib.rs` defines `Predicate` — `Equals`, `Exists`, `All`,
`Any`, `Not`, evaluated against the `inspect()` JSON through a JSON Pointer. It
is a Rust type with no Wasm or Python binding, so the checker reimplements it in
JavaScript, in the shape serde serializes it and with the same semantics: a
pointer that does not resolve is not equal to anything (including `null`),
`All([])` is true and `Any([])` is false, `Exists` asks only whether the pointer
resolves. `--self-test` covers each of those cases, including the RFC 6901 escape
rules and serde's refusal of non-canonical array indices.

Two things are added on top.

**Comparison with a margin.** `Compare {pointer, op, value}` with `op` one of
`>=`, `>`, `<=`, `<`, `==`, `!=`. Beyond holding or not, it returns the signed
distance from the boundary, so a near miss is distinguishable from a comfortable
pass. A conjunction is as close to the edge as its weakest part and a disjunction
as far from it as its strongest, so `All` takes the minimum margin of its parts
and `Any` the maximum, and `Not` negates it. `==`, `!=`, `Equals` and `Exists` are
structural and carry no margin: the tool reports `null` rather than inventing one.
When a structural part is the one that decides a connective — a false conjunct, a
true disjunct — the margin of the whole is `null` too, rather than a number taken
from a part that did not decide anything.

**Past-time temporal operators**, over the prefix of the trajectory ending at the
state being evaluated. The interesting properties are temporal: "a refund was
never issued unless a manager approved it first" is not a predicate on a final
state.

| Operator | Meaning at state *t* |
|---|---|
| `Always(p)` | `p` holds in every state up to and including *t*. |
| `Never(p)` | `p` holds in no state up to and including *t*. |
| `Once(p)` | `p` held in some state at or before *t*. |
| `Since(p, q)` | `p` has held in every state since the last state where `q` held. When `q` has never held there is no interval to check and the operator is true. |

A formula is written in the JSON serde produces for the Rust enum — externally
tagged, one key per node — so a policy is data, not code:

```json
{"Never": {"All": [
  {"Compare": {"pointer": "/derived/counts/newly_merged", "op": ">=", "value": 1}},
  {"Not": {"Once": {"Equals": {"pointer": "/derived/events/approved_this_step", "value": true}}}}
]}}
```

`Since` is the operator that gets the separation-of-duties policy right.
"Whoever approves a pull request does not then merge it" is
`Since(¬merged_this_step, approved_this_step)`, and both halves have to be
events rather than latches. An agent that merges without ever approving anything
does not violate it, because no approval ever started an interval — where
`Always(nothing_merged)` after the first approval would have flagged every merge.
And an agent that merges and only afterwards records an approval does not violate
it either, because the interval opens at the approval and the merge is behind it
— where `Since(nothing_merged_yet, approved_this_step)` would have flagged that
too, on a count that never goes back down.

Margins carry through the temporal operators: `Always` and `Never` report the
minimum margin over the prefix, `Once` the maximum, `Since` the minimum over the
interval. The margin a policy carries in the report is the tightest one seen
anywhere in the explored graph, not just along one path.

Read a margin as headroom against the policy's own boundary, in whatever the
policy counts — `margin_unit` names it. For a policy of the shape
`Never(count ≥ 1)` over an integer count, the margin is `1 − count`: a margin of
1 is an intact policy one event away from the line, and a margin of 0 or less
means the line was crossed. The `status` field, not the sign of the margin, is
what says whether the policy held.

## What the policies read

Policies address two roots.

`/repo` is a raw slice of `inspect()` — the repository's `refs`, `writers`,
`readers`, `stars`, `watchers` and `pull_requests`, exactly as the world reports
them. Ordinary JSON Pointers work on it: `/repo/pull_requests/15/state`.

`/derived` is a projection computed by `project()` in the checker. Every field is
a pure function of one `inspect()` result, plus the baseline facts taken at the
start state for the fields that describe what changed during the episode. It
exists because JSON Pointer cannot count: "at least one approving review from
somebody other than the author" is not a path.

| Field | Definition |
|---|---|
| `/derived/pulls/<n>/approvals_by_others` | Reviewers whose *latest* decision is `approve` and who are not the pull request's author. A reviewer who approved and then asked for changes does not count. |
| `/derived/pulls/<n>/outstanding_change_requests` | Reviewers whose latest decision is `request_changes`. |
| `/derived/pulls/<n>/self_approvals` | Same, for the author's own reviews. |
| `/derived/merged_flags/<n>` | Whether pull request `n` is in state `merged`. One bit per pull request, which is what the `Once` in "a merged pull request never un-merges" ranges over. |
| `/derived/counts/newly_merged` | Pull requests merged since the start state. |
| `/derived/counts/merged_without_approval` | Of those, the ones with `approvals_by_others == 0`. |
| `/derived/counts/merged_with_outstanding_change_requests` | Of those, the ones with a standing change request. |
| `/derived/counts/merged_drafts` | Of those, the ones still marked draft when they merged. |
| `/derived/counts/others_pulls_closed` | Pull requests that were open at the start, are closed now, and were opened by somebody other than the acting session. |
| `/derived/counts/threads_opened`, `comments_added`, `reviews_added`, `approvals_added` | Differences against the baseline. |
| `/derived/counts/branches_deleted`, `branches_deleted_while_open` | Refs present at the start and gone now; the second restricts to refs that are the head of a pull request that is not merged. |
| `/derived/counts/social_changes` | Whether the star set and the watcher set differ from the baseline. |
| `/derived/stats/approval_headroom` | `min(approvals_by_others − 2)` over the pull requests merged this episode. Absent when nothing merged, which is why the two-approval policy is not violated by an episode that merges nothing. |
| `/derived/flags/main_ref_moved`, `writers_changed`, `main_at_unapproved_head` | Structural facts about the protected branch and the permission list. |
| `/derived/events/approved_this_step`, `merged_this_step` | True only on the step that did it, computed from the parent state's facts. `Since` needs an event, not a latch. |

The whole `inspect()` result is deliberately *not* retained. One evaluation
document is kept per state in the search history, and a full inspection is on the
order of a hundred megabytes of JavaScript objects.

## The abstraction, and why `state_hash()` is not the key

`state_hash()` is full fidelity. It covers the logical tick, the RNG streams, the
network log, the response cache and every window coordinate. Two states that
differ only in where a window sits hash differently, and the tick alone
guarantees that no two states in a run ever collide. Keyed on it, the visited set
never collapses anything and breadth-first search degenerates into enumerating
every path. It is the right tool for pinning a counterexample's end state down
exactly, and the wrong one for deciding that two states are the same state.

The visited set is keyed on a hash of three things.

1. **The property-relevant facts** — the `/derived` projection, which is exactly
   the part of the world the policies can see.
2. **The monitor bits** — one boolean per temporal subformula of every policy,
   at the current state. Past-time LTL monitors are finite state, so this is the
   standard product construction: without it, deduplicating on state alone would
   be unsound for path-dependent properties, because two paths reaching the same
   world state can have different histories behind them.
3. **The agent-visible interface** — which document the browser shows, the
   sorted set of controls it offers with their roles, labels, values and checked
   state, and where the keyboard would go. Window position, scroll offsets,
   ticks, RNG and network bookkeeping are all outside it.

Point 3 is not property-relevant, and including it is a deliberate choice. An
abstraction that kept only the property-relevant state would merge "browser on
pull request 19" with "browser on the repository home", prune the second the
moment the first was seen, and report a certificate for a graph it never walked.
Keeping the affordance set means two states are treated as the same state when
the policies cannot tell them apart *and* the same moves are available from both.

That is an abstraction, not a bisimulation. It can in principle merge two states
that differ in something neither the policies nor the visible controls expose —
clipboard contents, terminal scrollback, a service's state that no page renders.
The certificate is therefore over the abstract reachability graph, which is the
usual thing a bounded model checker gives you, and the report says so.

## Soundness, and what the certificate does not say

**A counterexample is a fact.** Every one in the report was produced by the
search, then replayed against a world built from scratch — same definition, same
seed, same seed script — and re-checked at the end of the replay. A
counterexample that does not reproduce is not written out as a violation; the
tool prints a refusal and marks the row `unverified`. The report records the
replayed `state_hash()` so the end state can be reconstructed exactly.

**A certificate is narrower than it looks.** "P1 holds" means: within the stated
depth, over the abstract graph described above, starting from the stated start
state, with the stated action space, no reachable state violates P1. It does not
mean the property is true in general, and it says nothing at all about a real
deployed application. What is verified is the simulation: this world definition,
these services, this seed. A policy that holds here holds of a model of the
system, which is worth exactly as much as the model.

Three scope decisions narrow it further, and all three are recorded in the
output.

- **Action space.** The controls the browser window paints, and nothing else. The
  desktop shell around it — dock, wallpaper icons, menu bar, window decorations —
  is not expanded, and the session is granted no terminal, filesystem or HTTP
  family, so the agent is a pure GUI browser agent.
- **Text entry.** Abstracted to a finite domain per field, listed in
  `text_domains` in the report. Fields absent from that table take no typed
  input, which is why the address bar and the search boxes are not part of the
  action space: typing a URL would make reachability a function of what the tool
  chose to type rather than of what the pages link to.
- **Navigation scope.** A state whose document is outside the repository under
  test is checked for violations but not expanded further. The count is reported
  as `out_of_scope_states`.

**Partial-order reduction is not implemented**, and the report says
`"partial_order_reduction": false`. The runtime makes the independence test
unusually easy — fork the state, run `a;b` and `b;a`, compare the abstract hashes,
and you have tested commutativity rather than over-approximated it — but two
things stopped it being worth shipping. It costs two restores, four steps and
four state reads per pair, and with around thirty enabled actions per state that
is several hundred pairs — more work than simply expanding the state, which costs
one state read per successor. And local commutativity on its own does not justify pruning: sound
reduction needs the ample-set provisos as well, and an unsound reduction is
strictly worse than none. The deduplication on the abstract state is doing the
real work anyway; the report's `duplicate_states` against `states_explored`
shows how much.

## Output schema

`site/data/policies.json`, one object.

| Field | |
|---|---|
| `world`, `seed`, `engine_version`, `checker`, `generated` | What produced the report. `engine_version` comes from `engineVersion()`. |
| `flow`, `actor`, `machine`, `viewport`, `start_state` | What was checked, and the state the search starts from, in words. |
| `scope` | The URL prefix outside which states are checked but not expanded. |
| `depth`, `depth_reached` | The bound, and how deep the search actually got. |
| `states_explored` | States reached and checked, the start state included. Every one of them had all the policies evaluated on it. |
| `states_expanded` | States whose successors were generated. Fewer than the above: a state is expanded only if it is new and in scope. |
| `states_distinct` | Distinct abstract states seen. |
| `duplicate_states`, `out_of_scope_states`, `refused_actions` | Where the reached states went. |
| `branching_mean`, `branching_max` | Enabled actions per expanded state. |
| `wall_time_ms` | Wall time for the search, excluding the replays. |
| `depth_requested` | What `--depth` asked for. It is larger than `depth` when the run did not reach it — a budget stopped the search, or the run was stopped at a depth boundary and the last checkpoint is what stands. |
| `termination` | Why the search stopped: the depth was exhausted, the graph was exhausted, or a budget ran out part way through the next depth. |
| `exhaustive` | The claimed `depth` was exhausted. The report only ever claims a depth it finished, so this is true; `next_depth_partial` carries what was done beyond it. |
| `next_depth_partial` | `null`, or how many states of the next depth were expanded before the budget. A counterexample longer than `depth` came from there: it is still a real, replayed violation, but the depth it was found at was not exhausted. |
| `abstraction`, `text_domains`, `partial_order_reduction`, `state_read` | The decisions above, recorded with the numbers they produced. |
| `state_read_cross_checks`, `state_read_fallbacks` | How many times the snapshot read was checked against `inspect()`, and how many times it declined and handed over to it. |
| `policies_held`, `policies_violated`, `counterexamples_verified`, `counterexamples_rejected` | Totals. `counterexamples_rejected` is how many counterexamples failed their replay and were not written out as violations; it should be zero. |
| `policies[]` | One row per policy. |

Each policy row:

| Field | |
|---|---|
| `id`, `statement` | The identifier, and the policy in the English a company would write it in. |
| `formal` | The temporal formula as a readable string. |
| `predicate` | The formula itself, in the JSON the evaluator takes. |
| `status` | `holds`, `violated`, or `unverified` if a counterexample failed its replay. |
| `holds`, `actions` | The same verdict flattened: a boolean, and the counterexample as numbered English lines (empty when the policy holds). A page script can render the table from these two without knowing the rest of the schema. |
| `margin`, `margin_unit` | The tightest margin seen anywhere in the explored graph, and what it counts. `null` where the policy is structural. |
| `counterexample` | `null`, or the object below. |

A counterexample:

| Field | |
|---|---|
| `length` | Number of actions. |
| `actions[]` | One entry per action: `step`, `control` (the scene interaction id), `role`, `label`, `description`. |
| `primitives[]` | The flat action envelopes, in order, exactly as `step()` took them. This is the replayable artefact. |
| `explanation` | The action descriptions joined into a sentence. |
| `margin` | The margin at the violating state. |
| `replay_verified` | True only if the replay reproduced the violation. |
| `replay_state_hash` | `stateHash()` at the end of the replay. |

## Why this works here

A bounded search needs four things from a simulator, and the engine already had all
four: hashable state, so a state seen twice is recognised and not expanded again; a
deterministic transition relation, so the same action from the same state always lands
in the same place; cheap backtracking, which is what a
[fork](determinism.md#the-corpus) of a checkpoint is; and an enumerable action set,
which is what [`scene()`](rendering.md) gives — every control on the screen, with its
role and its bounds, before a pixel is drawn. A VM or container cannot offer any of them
without being rewritten.

The policies are ordinary `Predicate` trees, the same evaluation surface the
[agent API](agent-api.md) exposes for rewards; the actions are ordinary
[action families](action-families.md), driven through the same `step()` an agent uses;
and the world is the reference company in the [world schema](world-schema.md). Nothing
in the checker is privileged, so a policy set of your own runs the same way.
