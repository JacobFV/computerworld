# Document service (`docs`)

Register `cw_service_docs::DocsService` or `register(&mut Registry)`.

Seed: `{"documents":{},"next_id":0}`. Seed documents use fields `id`, `title`,
`owner`, `readers`, `writers`, `body`, `revision`, `history`, `comments`.
Defaults fill optional fields. An optional `body_variants` array selects one
body using the instance seed, then is removed from initialized state. This
supports deterministic semantic world variation. Initial revision history
captures the selected body.

| Route | Behavior |
| --- | --- |
| GET `/` | Readable document links and create form |
| GET `/documents/{id}` | Native document, editor and comments |
| GET `/api/documents` | Accessible documents |
| POST `/api/documents` | `{title,body,readers:[],writers:[]}` |
| GET `/api/documents/{id}` | Authorized document/revisions/comments |
| PUT/PATCH/POST `/api/documents/{id}` | `{revision:1,body:"Updated"}` |
| POST `/api/documents/{id}/comments` | `{text:"Review"}` |

Writes require owner/writer and exact current revision. Stale writes return
409 without changing state. Readers can comment; inaccessible documents are
not listed. Native forms use the same endpoints without `/api` and return
updated pages. Page projection does not mutate state. Link text is interpreted
as explicit navigation controls, never fetched during rendering. Successful
mutations emit `docs.mutated` effects.

Extends SynthUX's actual Python document writes with revisions, grants and
comments while replacing its disconnected read-only website store. This is a
text document editor, not a full rich-text compatibility implementation.
