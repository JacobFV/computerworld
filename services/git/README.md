# Synthetic Git service

Register `cw_service_git::register(&mut registry)` and place a service of kind
`git` on any network node. The kernel has no dependency on this package.

Initial state:

```json
{"repositories":{"onboarding":{"files":{"README.md":"Welcome"},"writers":["alice","bob"]}}}
```

`files` seeds an initial commit on `refs/heads/main`; alternatively provide
`objects` and `refs`. Empty or absent ACLs mean public. Nonempty `readers` restrict
reads; writers may also read. Nonempty `writers` restrict pushes. Actor identity
comes from the service context, never a caller-selected request body/header.

| Route | Method | Result |
|---|---|---|
| `/` | GET | Native page repository list |
| `/repos/{repo}` | GET | Native page refs and committed files |
| `/api/git/repos` | GET | Visible repository names |
| `/api/git/repos/{repo}` | GET | `{objects,refs}` clone/fetch payload |
| `/api/git/repos/{repo}/refs` | GET | Reference map |
| `/api/git/repos/{repo}/objects` | GET | Commit object map |
| `/api/git/repos/{repo}/push` | POST | Atomic object/ref update |

A push is `{objects:{hash:commit},refs:{"refs/heads/main":hash},expected_refs:{"refs/heads/main":oldHash},force:false}`.
`expected_refs` is optional optimistic concurrency control; an empty old hash
asserts that the ref does not exist. Existing refs must fast-forward unless
`force:true`. Every submitted object's SHA-256 ID and parents are checked before
any mutation. Failure leaves service state unchanged.

The canonical commit is the compact UTF-8 JSON encoding of this **field order**:
`parents`, `files` (lexicographically sorted map), `message`, `author`, `tick`.
Object ID is lowercase SHA-256 of those bytes. This is a synthetic Git protocol,
not native Git packfiles or Git smart HTTP. It preserves clone/fetch/push,
content identity, commit ancestry, branches, atomic refs and distinct working
trees while remaining portable to browser Wasm. Trees currently contain UTF-8
text. The service receives no local working-tree paths or host files.

Statuses: 400 malformed JSON; 403 ACL denial; 404 missing route/repository;
405 wrong method; 409 stale/non-fast-forward ref; 422 bad hash, ref or parent.
