# Mail service (`mail`)

An independently addressable SDK service with serialized instance state. Register
`cw_service_mail::MailService`, or call `register(&mut Registry)`.

Seed: `{"users":["alice","bob"],"messages":{},"next_id":0}`. Actor IDs are
mailbox addresses; display email addresses can also be used as IDs. Sender
identity always comes from `ServiceContext.actor`, never JSON or headers.

| Route | Behavior |
| --- | --- |
| GET `/` | Native mailbox and compose form |
| GET `/api/messages?folder=inbox` | Actor's visible messages; optional folder filter |
| POST `/api/messages` | `{to:["bob"],cc:[],subject:"Hello",body:"Text"}` |
| PATCH/POST `/api/messages/{id}` | `{read:true,label:"work",archive:true}` |

Native forms use `/send` and `/messages/{id}` with identical semantics and render
the resulting mailbox. Compose atomically validates all recipients before
creating one shared message with per-user inbox/sent metadata. Self-delivery
preserves both folders. Labels/read/archive affect only the acting mailbox;
API responses redact other users' private metadata. Unknown recipients fail;
there is no ambient SMTP or outbound network. Opening pages is a pure read;
mark-read is explicit. `mail.mutated` events describe accepted mutations.

This preserves SynthUX Python store delivery and ownership semantics, repairing
self-mail folder loss and labels without ownership checks. It replaces the
standalone mock's sender-only Sent insertion. No predecessor code was copied.
