# Calendar service (`calendar`)

Register `cw_service_calendar::CalendarService` or `register(&mut Registry)`.

Seed: `{"events":{},"next_id":0}`. Event fields are `id`, `owner`, `title`,
`start`, `end`, `created`, and `attendees` mapping actor IDs to RSVP values.
All time is integer logical microseconds; no date parsing, host timezone or
ambient clock is involved. The world may define an epoch for display.

| Route | Behavior |
| --- | --- |
| GET `/` | Visible events and create/move/RSVP forms |
| GET `/?view=month&day=<d>&event=<id>&hide=<owners>` | Skinned only: which grid, which day, which event is open, and which calendars the reader has unticked. Every link on the page carries them, so a page is a permalink and a filter survives a click |
| GET `/events/{id}` | Skinned only: one event's permalink, in the week it falls in |
| GET `/api/events?start=0&end=10000000` | Accessible events overlapping half-open interval |
| POST `/api/events` | `{title,start,end,attendees:["bob"]}` |
| PATCH/POST `/api/events/{id}` | Owner-only `{start,end}` reschedule |
| DELETE `/api/events/{id}` | Owner-only deletion |
| POST `/api/events/{id}/rsvp` | `{response:"accepted"}` |

Responses are pending/accepted/declined/tentative. Only invited actors may
RSVP. Only owners and invitees can see events. Durations must be positive.
Native forms omit `/api`, share the authoritative store and return updated
pages. `calendar.mutated` effects enter the trajectory.

Preserves causal create/move/delete/respond behavior from SynthUX's Python
store, adds missing ownership checks, and eliminates the standalone calendar's
ambient Date/timezone initialization. Recurrence expansion and civil-time
formatting are intentionally not implemented by this initial service package.
