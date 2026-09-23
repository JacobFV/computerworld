# Slack service (`slack`)

A Slack workspace with Slack's semantics: channels with a topic and a purpose, direct
messages between two people or a small group, threads under a message, reactions, pins,
a member directory with display names, titles and statuses, an unread count per person
per conversation, and @-mentions. Register `cw_service_slack::SlackService` or
`register(&mut Registry)`. The reference world binds `worlds/internet/services/slack/service.json`
to it (`slack.com`, `app.slack.com`, `northstar.slack.com`).

Seed:
```json
{"workspace":"Northstar","next_id":2,
 "channels":{"eng":{"title":"eng","topic":"CI and reviews","purpose":"Engineering","members":["alice","bob"],
   "messages":[{"id":"chat-1","author":"bob","text":"Windows CI is red","time":30,"reactions":{}},
               {"id":"chat-2","author":"alice","text":"looking","time":31,"reactions":{},"parent":"chat-1"}],
   "pins":["chat-1"]}},
 "dms":{"alice|bob":{"title":"alice, bob","members":["alice","bob"],"messages":[]}},
 "members":{"alice":{"display_name":"alice","title":"Software Engineer","status":"🚀 launch week"}}}
```

- Membership scopes everything: a channel or DM is only there for the people in it. A
  channel with `"private": true` shows a lock instead of a hash.
- A DM is keyed by its members sorted and joined with `|`; three or more make a group DM.
- `seen` (person to conversation to the number of messages they had seen) is what
  unread counts are measured against; opening a page is a GET and changes nothing, so
  reading is its own POST.
- A message that writes `@name` mentions that person; the sidebar badges their unread
  mentions per channel, and the rail's Activity tab counts them.
- `time` is on Slack's clock: microseconds since `HISTORY` (seven days) before the
  world's epoch of 2026-09-17 09:00, so a seed can hold the week before the world
  starts. A message sent at world tick `t` is stored at `t + HISTORY`. The page shows
  clock times ("9:41 AM") and puts "Today", "Yesterday", a weekday or a date over each
  day, seen from the later of the world clock and the newest message.

The page is HTML (`text/html`, built on `cw_service_common::html`, styled by
`src/slack.css`) in Slack's desktop layout: the aubergine frame with the search box in
its top bar, the rail of Home, DMs and Activity, the sidebar of channels
and DMs (unread in bold, the open one in blue), the conversation with messages grouped
by author under date dividers, emoji short names as characters, mentions tinted, code
in the monospace face, reactions as chips that react when pressed (the caller's own
outlined in blue), threads collapsed to "N replies · Last reply 2h ago", a toolbar that
floats over the message under the pointer (the last message keeps its own showing), and
the composer under the transcript. Every control on the page acts: what cannot —
Slack's emoji picker, its formatting strip, the attach and apps buttons, Later and More
— is not drawn rather than drawn dead, and `tests/controls.rs` crawls every page kind
to keep it that way. It is an app shell: the body is a full-height flex
column, and the sidebar list, the transcript (`#messages`) and the pane's list scroll on
their own. `?thread=<message id>` opens that thread in a pane on the right with its own
reply field; `?members=1` opens the member list there. A seeded `theme` overrides the
palette through custom properties on `<html>`.

Ids are the agent API: `nav-<channel>`, `dm-<key>` (links), `start-<person>` (a button
in a one-field POST form to `/dms`), `send` / `send-text` / `send-submit`,
`<message>-react-<emoji>` and `<message>-quick-<emoji>` (buttons carrying
`reaction=<emoji>`), `<message>-pin` (`formaction` to the pin route),
`<message>-open-thread` and `<message>-replies` (links to `?thread=`),
`<parent>-reply` / `-reply-body` / `-reply-submit`, `read` / `read-submit`,
`channel-members`, `thread-close`, `members-close`, `search` / `search-q` /
`search-go` (the top bar's search form), `rail-activity` (the Activity tab).

| Route | Behavior |
| --- | --- |
| GET `/`, `/channels/{id}`, `/dms/{id}`, `/archives/{id}` | The workspace: rail, sidebar with unread badges, the open conversation with its topic, threads, pins and reactions; `/` and `/dms` open the first channel or DM. `?thread=<id>` and `?members=1` open the right-hand pane |
| GET `/search?q=` | What the top bar's search box finds: every message of a conversation the caller is in whose text holds the query, newest first |
| GET `/activity` | The Activity tab: the caller's unread @-mentions, newest first |
| GET `/api/channels`, `/api/dms` | Membership-filtered ids to titles |
| GET `/api/channels/{id}` (`/messages`) | The conversation plus the caller's `unread` |
| GET `/api/channels/{id}/pins` | The pinned messages |
| GET `/api/members`, `/api/mentions`, `/api/unread` | The directory; the caller's unread mentions; the caller's unread counts |
| POST `/api/channels/{id}/messages` | `{text, parent?}`: a message, or a threaded reply |
| POST `/api/channels/{id}/messages/{message}/reactions` | `{reaction:"+1"}`, idempotent per person |
| POST `/api/channels/{id}/messages/{message}/pin` | Pin, or unpin if pinned |
| POST `/api/channels/{id}/read` | The caller has seen everything there |
| POST `/api/dms` | `{to:"bob"}` or `{to:["bob","carol"]}`: open (or find) the DM |
| POST `/api/status` | `{status}`: the caller's status line |

Forms use the same paths without `/api` and land on the conversation they changed.
Every successful POST emits `slack.mutated`. The page module (`src/page.rs`) is the
whole rendering, kept separate from the state so the look can be reworked on its own.
