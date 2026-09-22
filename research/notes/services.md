# SynthUX standalone service archaeology

Inspected complete git histories and all service/state modules, rendering entry points, README/package manifests at the following immutable revisions (2026-09-17 checkout). All six repositories have Node >=18, zero npm dependencies, private packages, `dev`/`start` scripts only, and no checked-in tests, CI, license file, or package license field. Absence of license means code/assets should not simply be relicensed; reuse observed semantics and independently express Rust implementation, with provenance retained. No AGENTS.md exists in these source trees or the destination workspace.

| Repository | HEAD | History |
|---|---|---|
| jacobfv/synthux-github-mock | `595dbdef57e3080952f5c0bd9625156d2323edf9` | 3 commits: initial `1a5448c`, README parent link `0b0e9d1`, interactive revision `595dbde`; all May 27 2026 |
| jacobfv/synthux-mail-mock | `a25f2601e350d04ad6b6bb293a25c20f9b72b7d6` | One initial commit, May 27 2026 |
| jacobfv/synthux-calendar-mock | `9333e1de4168a92f5219a5aa721f460c58959825` | One initial commit, May 27 2026 |
| jacobfv/synthux-docs-mock | `6ca552c8aa705efa0eebb46ee13d0e6ed02a23f3` | Initial `de27211`, then README parent link; May 27 2026 |
| jacobfv/synthux-jira-mock | `f367004040a4d9ed9f3f014383d777a4a9a66f43` | One initial commit, May 27 2026 |
| jacobfv/synthux-slack-mock | `378b8c819e2605f93d36960729d5310dc2b1b25a` | One initial commit, May 27 2026 |

## Provenance matrix (service scope)

“Best” here compares these standalone predecessors, pending comparison to unified engine service implementations.

| Subsystem | Best current implementation | Source repo/path | Important alternatives | What should survive |
|---|---|---|---|---|
| Independent service boundary | Separate HTTP service, no OS dependency | All six `src/server.mjs` | SynthUX reverse-proxy mounted services | Addressable service instances with request/response contract and isolated persistent state, but in-process deterministic execution |
| Git hosting UI/domain state | Latest interactive GitHub mock | `synthux-github-mock/src/state.mjs:287-400`, `server.mjs:53-196` | Initial revision was almost wholly static rendering | Repo file-tree/raw views; issue/PR creation; comments/review transitions; scoped repo state; clear separation of storage and rendering |
| Document schema | Structured document blocks | `synthux-docs-mock/src/server.mjs:14-82`, `render.mjs:85-110` | Read-only HTML page façade | Heading/paragraph/callout/todo/bullet block vocabulary; nested page identities, no DOM required |
| Mail schema | Thread/message/folder/label state | `synthux-mail-mock/src/state.mjs:1-137` | Compose only writes sender Sent | Thread/message vocabulary, folders/labels, create transaction; add actual mailbox ownership/delivery |
| Calendar schema/layout | Event/date/calendar state and time-grid rendering | `synthux-calendar-mock/src/state.mjs`, `render.mjs:92-184` | Host Date/timezone initialization | Typed event records, date/week filtering and geometry, reexpressed against deterministic calendar/time |
| Issue tracker | Project/issue records and board grouping | `synthux-jira-mock/src/state.mjs`, `server.mjs:47-87` | No issue update/comment/status transition routes | Project keys, status groups, per-project monotonic numbering, create/validate transaction |
| Chat | Workspace/channel/DM scene/fixture data | `synthux-slack-mock/src/render.mjs` | POST explicitly drops all data | Workspace/channel/message view vocabulary only; implement actual shared mutable chat state |

## Real capability versus façade

### Shared properties

These are standalone HTTP HTML renderers, not network stacks or complete application backends. Each server binds a real localhost socket (ports 5180–5185), uses environment variables for host/port, and serves its own HTML/CSS. They have no DNS/domain ownership, topology, authentication/session identity, capability authorization, storage adapter, serialization, reset, seed argument, scheduler, event log, replay, or rasterization/structured observation API. Domain mapping lives in the parent browser/proxy, not service modules. Their useful independence from a particular OS should survive; their process/socket requirement should not.

No service issues outbound `fetch` or imports a host filesystem module. They do not themselves fall through to internet access. Shared mutable state is module singleton state; two clients see the same data only because they hit the same Node process. Two episodes in one JS process cannot instantiate independent service state. There is no disk durability. All seeds are fixed content, except calendar's ambient current week. There is no seeded randomness. Most displayed timestamps are fixed prose such as “just now,” not semantic clock values.

HTTP forms use urlencoded bodies and redirects (usually 303); health checks return JSON. The normal service surface does not offer JSON domain APIs. Request buffers have no size limit; these are illustrative mocks, not production host services. Do not carry process-global Maps, Node request streams, permissive iframe headers, module-load seeding, or blanket success/fallback responses into kernel contracts.

### GitHub

`595dbde` is a substantial evolution: adds 400-line `state.mjs`, `highlight.mjs`, tree/blob/raw routes, mutable issues/PRs/reviews, and form rendering. It is not merely a wrapper around the initial HTML. `getRepo` lazily creates any requested owner/repo with identical seed files/issues/PRs; scoped Maps isolate mutations by repo name. `listTree` implements sorted directory-first prefix grouping. New issue IDs start at 200 and PR IDs at 100; review actions change PR state to `approved`/`changes-requested` rather than modeling separate review and open/closed dimensions.

No git object store, branch contents, commit graph, clone/fetch/push protocol, merge operation, or file editing exists. `/tree/<branch>` and `/blob/<branch>` ignore branch value. Commit/branch/tag totals are static counters. Diff preview is hardcoded for selected seed PRs, not computed from source state (`render.mjs:448-487`). Actions/search are placeholders. “Close issue” button posts `close=1`, but server ignores it and adds a comment (`render.mjs:637`, `server.mjs:99-102`). Missing issue/PR mutation redirects anyway. Arbitrary non-POST methods can reach GET logic. Preserve interactions that work, explicitly add causal git semantics elsewhere.

### Mail

One global mailbox, fixed alice sender, eight initial threads. `createSentThread` creates a new record in `folders:['sent']`; it never delivers to recipients, updates an existing thread, or connects a mail transport (`state.mjs:124-137`). Reply/forward links carry query parameters the server ignores. Opening a thread does not mark it read; unread is a static stored flag with no transition route. Displayed folders/labels do not imply move/delete/star/snooze operations. No email auth, attachments, draft edits, search, or per-user mailbox separation.

### Calendar

Fourteen weekly events, five calendars, create/list/week/day/detail views. Seeds derive from `new Date()` at module import (`state.mjs:39-41`); local calendar operations use host timezone. Rendering also reads current Date. New-event defaults use ambient today. Title is required; dates/times/calendar IDs are not robustly validated. No edits/deletes/invitations/recurrence/conflict detection. Event geometry is valuable reference: time to y position and duration to height, but `Math.max(20, duration*48-2)` is UI policy rather than simulator semantics. Preserve explicit timestamps/timezone in world data and clock-controlled highlights.

### Docs

Three hardcoded ACME pages; nested route identities and structured block data are genuinely reusable concepts. Store and fixtures are in `server.mjs`; no write path exists. Share/Updates controls and some sidebar links are decorative. Server does not filter HTTP methods, so POST to a page returns 200 with unchanged page, not an edit. Do not describe this predecessor as a working document editor.

### Jira

Three projects, eight seeded ACME issues, four board statuses. `nextNum` scans project issues for max + 1. POST creates backlog issue with fixed reporter alice, user-chosen summary/type/priority/assignee, comma-separated labels. It validates existing project and nonempty summary. No edit, transition, comment, assignment, sprint, or board drag operation exists. Fields and board grouping survive; actual state transitions require new semantics.

### Slack

Workspace/channel/message fixtures live in `render.mjs`; channel views and DM views render static content. `server.mjs:49-55` explicitly reads and discards every POST body, then redirects to Referer or `/`. The working-looking compose controls are not working chat. No auth, message delivery, shared persistent chat mutation, reactions, search, or workspace creation. Only information architecture and fixture/view examples are reusable.

## Cross-service consistency defects

Mail (`state.mjs:40-47`), Jira (`state.mjs:39`), and Slack (`render.mjs:51`) mention GitHub PR #4821. Both the initial GitHub list and latest actual state seed PRs 42, 41, 40, 38, 35. The old detail renderer accepted arbitrary numbers, hiding mismatch; latest state lookup exposes missing records. These are matching narratives, not a causally consistent ecosystem. Dates/incident accounts differ across docs, Jira, calendar, and mail. Build a single reference-world data package with validated foreign references rather than copying fixtures individually.

## Executed probes

Ran source module probes under installed Node: issue creation returned 200; comment persisted; created PR approval set state `approved`; second repo retained independent initial state; GitHub seed PR IDs were `[42,41,40,38,35]`. Mail composition produced `th-9` in Sent only, sender alice, with recipients as data.

Launched disposable local Slack and Docs servers and issued POSTs containing a unique sentinel: Slack redirected to `/`, final 200, sentinel absent; Docs returned page 200, sentinel absent. Both child processes were terminated. These verify façade behavior; they are not comprehensive service tests. No tests shipped upstream. Node module probing changes only process memory, not repositories.

## Migration recommendation

Define pure `Service::handle(context, request)` transitions over serializable instance state and emit response plus semantic events. Supply clock/RNG/identity and capability-checked synthetic networking in context; never give a service a Rust host socket or process handle. Service-owned endpoint/domain routing should be separate from browser navigation. Service response can contain typed page schema/asset references or API data; browser translates page schema into application/scene state without parsing HTML. Request transitions mutate, scene projection does not. Cross-computer tests must mutate through one computer and observe through another.

Migrate data schemas and working transitions, author structured scenes with visual references from old HTML, add explicit new behavior required by examples (mail delivery, doc edit, chat send, git fetch/push, Jira transition). Label additions as new, not recovered features. Capture old output for visual and navigation regression; do not use decorative controls as a correctness oracle. A temporary optional Node compatibility fixture runner is acceptable for comparison, never canonical runtime semantics.

## Verified parent lineage and stronger inline semantics

SynthUX `81f2f320012f4e2724ca469bebec99193d395e79` uses git submodules (not vendored source copies). `git ls-tree HEAD sim/services/` pins **all six exactly to the standalone HEADs above**, so there are no parent-only service fixes. History: `0e0c48ae7e65651694716544a2815eafca89d603` introduced GitHub/docs via local bare repos, `18cf640e065e3f571edae366d3f58ac5095288dd` published their same histories and switched remotes, `c30df17` advanced GitHub to interactive `595dbde`, `ef56e45` added the other four service submodules.

The strongest mail/chat/docs **behavior** in SynthUX is not in these standalone mocks:

- `src/synthux/world/mail.py:MailStore` implements sender-origin validation, recipients + cc delivery, participant-scoped search, per-user labels/archive, monotonic message IDs, and recorded world events. `internet.py:146-180` exposes send/inbox. Labels/archive do not themselves verify actor ownership, so do not treat all operations as an authorization model.
- `src/synthux/world/chat.py:ChatStore` implements channel membership, actor-origin posting, shared history, monotonic message IDs, reactions, and world events. `internet.py:43-144` exposes these but `ensure_chat_channel` automatically adds posting actor to membership and accesses private storage, weakening direct-store membership semantics. Reactions check actor-origin but not channel membership.
- `internet.py:183-196` has actual document write/read and `internet.docs.written` events over a Python dict; separate from standalone read-only rich-block docs.
- `internet.py:222-230,663-969` has a static inline GitHub fallback; standalone latest has the richer issue/PR interactions.

These world-backed JSON APIs and the rendered standalone HTML services have **separate stores**, so sending mail via API does not update the Gmail-looking website, and chat posted through API does not appear in the Slack fixture. Recover Python causal domain transitions and standalone information architecture together; do not preserve this split.

The parent reverse proxy is incomplete for interaction: `internet.py:232-286` fetches upstream HTML by GET using urllib, with no link/form rewriting. `make_handler.do_POST:384-393` parses only JSON and invokes API routes; it does not proxy service POSTs. Root-relative links/forms from standalone pages escape their `/web/<service>` mount. Upstream redirects are followed by urllib without fixing emitted root-relative links. Thus independent service form functionality demonstrated directly should not be assumed functional through SynthUX browser proxy. This is a migration defect to explicitly cover with integration tests, not a stronger compatibility behavior to retain.
