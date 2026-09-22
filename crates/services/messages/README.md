# Messages service (`messages`)

Texting the way a phone does it: iMessage and SMS between *handles* (an E.164 number
such as `+14155550100`, or an Apple-ID-style address), not Slack channels. Register
`cw_service_messages::MessagesService` or `register(&mut Registry)`. The native Messages
app in `cw-applications` (`crates/applications/src/apps/messages.rs`) talks to this
service at `http://messages.internal/`.

Seed:
```json
{"contacts":{"alice":{"name":"Alice Chen","handles":["+14155550100","alice@icloud.com"],"imessage":true},
             "bob":{"name":"Bob Martinez","handles":["+14155550101"]}},
 "conversations":{"+14155550100|+14155550101":{"participants":["+14155550100","+14155550101"],
   "messages":[{"id":"sms-1","from":"+14155550101","text":"lunch?","time":30,"service":"imessage",
                "delivered":["+14155550100"],"read":{"+14155550100":30}}]}},
 "next_id":1}
```

- `contacts` maps a simulation user to a display name and their handles; the first handle
  is the one they send from. `imessage: false` marks a number that is not registered, so
  texts to it go as SMS (green) and carry no receipts.
- A conversation is keyed by its participant handles sorted and joined with `|`, so any
  participant opening the same set of people lands in the same thread. Three or more
  handles make a group, which may carry a `name`.
- A message records `service` (`imessage` or `sms`), `delivered` (handles it reached;
  an iMessage is delivered to everyone else as it is sent, an SMS reports nothing),
  `read` (handle to the tick they read it) and `tapbacks` (`loved`, `liked`, `disliked`,
  `laughed`, `emphasized`, `questioned`, each to the handles that gave it).

| Route | Behavior |
| --- | --- |
| GET `/` | Phone-shaped conversation list, blue dot where something is unread |
| GET `/conversations/{id}` | The thread as bubbles (blue iMessage, green SMS, grey incoming) with the delivery or read status under the last outgoing text |
| GET `/api/conversations` | The actor's conversations, newest activity first: `{id,title,participants,service,unread,preview,time}` |
| POST `/api/conversations` | `{to:"bob"}` or `{to:["+14155550101","carol"],name:"Launch crew"}`: open (or find) the thread with those people; `to` accepts handles or contact user names |
| GET `/api/conversations/{id}` | The thread with `me`, `title`, `people` (handle to name), `service`, `unread` and `messages` |
| POST `/api/conversations/{id}/messages` | `{text}`: send; the service and delivery set come from the participants |
| POST `/api/conversations/{id}/read` | Read everything from others (an iMessage gets a receipt at the current tick) and answer with the thread |
| POST `/api/conversations/{id}/messages/{message}/tapbacks` | `{tapback:"loved"}`: give it, or take it back if already given |
| GET `/api/contacts` | The directory: `[{user,name,handles,imessage}]`; what the Contacts app reads |

Forms use the same paths without `/api`. The actor resolves to a handle through the
contacts; an actor with none can only look at an empty list. Only participants can read
or write a conversation. Every successful POST emits a `messages.mutated` effect into the
trajectory. Ids are `sms-N`; authors and times come from the simulation context.
