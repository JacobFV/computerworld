# Discord service (`discord`)

A Discord server with Discord's semantics: roles with a colour and a rank, categories of
text and voice channels, members with a nickname and roles, replies that quote what they
answer, reactions, and voice channels that have occupants rather than messages. Register
`cw_service_discord::DiscordService` or `register(&mut Registry)`. The reference world
binds `worlds/company-2026/sites/discord.json` (the Atlas Community server) to it.

Seed:
```json
{"next_id":1,"server":{"id":"atlas","name":"Atlas Community",
  "roles":{"Admin":{"color":"#f23f43","position":3},"Member":{"color":"#f2f3f5","position":1}},
  "categories":[{"name":"Information","channels":["welcome"]},{"name":"Community","channels":["general","Lounge"]}],
  "channels":{"welcome":{"topic":"Start here","messages":[]},
              "general":{"messages":[{"id":"chat-1","author":"alice","text":"hi","time":1,"reactions":{}}]},
              "mod-log":{"roles":["Admin"],"messages":[]}},
  "voice":{"Lounge":{"occupants":["praman"]}},
  "members":{"admin":{"roles":["Admin"]},"alice":{"nick":"alice.chen","roles":["Member"]}}}}
```

- Everyone in `members` can read every text channel unless the channel names `roles`,
  in which case one of them is required. Anyone else gets 403 for the whole server.
- A message may `reply_to` another in the same channel; the page draws Discord's reply
  line over it.
- A reaction is a toggle: reacting again with the same emoji takes yours back.
- A member is in at most one voice channel; joining another leaves the first.
- `time` is microseconds on Discord's clock, which runs seven days ahead of the world's
  (`cw_service_discord::HISTORY`) so a seed can span the week before the world starts;
  the page stamps messages "Today at 9:41 AM", "Yesterday at ..." or "09/15/2026 ...",
  and divides days. A message sent at world tick `t` is stored at `t + HISTORY`.

| Route | Behavior |
| --- | --- |
| GET `/`, `/channels/{server}`, `/channels/@me` | The server with no channel open |
| GET `/channels/{server}/{channel}` | The channel: dark layout, server rail, categories on the left, messages grouped by author under date dividers, the composer pinned at the bottom, members by role on the right (`?members=0` closes the list). `/channels/{channel}` is the short form seeded prose and scenes use |
| GET `/api/server` | Roles, categories, voice occupancy, members, and the channels the caller can see |
| GET `/api/channels` | The caller's visible text channels |
| GET `/api/channels/{channel}` (`/messages`) | The channel's topic, roles and messages |
| POST `/api/channels/{channel}/messages` | `{text, reply_to?}` |
| POST `/api/channels/{channel}/messages/{message}/reactions` | `{reaction:"eyes"}`, toggling the caller's reaction |
| POST `/api/voice/{channel}/join`, `/api/voice/{channel}/leave` | Move the caller into, or out of, a voice channel |

Every route also accepts the `/{server}/` segment after `/channels`. Forms use the same
paths without `/api`. Every successful POST emits `discord.mutated`.
