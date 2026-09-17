# Chat service (`chat`)

Register `cw_service_chat::ChatService` or `register(&mut Registry)`.

Seed:
```json
{"channels":{"general":{"title":"General","members":["alice","bob"],"messages":[]}},"next_id":0}
```

| Route | Behavior |
| --- | --- |
| GET `/` | Visible channel navigation |
| GET `/channels/{id}` | Native history, message/reaction forms |
| GET `/api/channels` | Membership-filtered channel IDs/titles |
| GET `/api/channels/{id}/messages` | Channel and persisted messages |
| POST `/api/channels/{id}/messages` | `{text:"Hello"}` |
| POST `/api/channels/{id}/messages/{message}/reactions` | `{reaction:"thumbs-up"}` |

Forms use equivalent paths without `/api`. All reads/posts/reactions enforce
membership. Posting never implicitly enrolls the actor. Repeating a reaction
is idempotent; reactors are a sorted set. Message authors and time come from
simulation context. State has no globals; cloned/serialized states isolate
worlds. `chat.mutated` effects enter the trajectory.

Reexpresses SynthUX's Python membership/history/reaction semantics while fixing
reaction access and implicit channel enrollment. Replaces the standalone
website's discarded POST bodies with actual persistent messages.
