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

## The `github` skin

`"skin":"github"` in the initial state dresses the same repositories as github.com:
repositories carry an `owner`, `description`, `topics`, `stars`, `forks`, `issues`
and `pull_requests` (one shared number space; a pull request has `head`, `base`,
`reviews`, optional `draft` and `merged_by`), and `gists` sit beside them. The
`/repos/*` and `/api/git/*` routes above stay byte-identical; everything else is
the owner-namespaced surface, every page derived from the commit graph:

| Route | Page |
|---|---|
| `/`, `/search?q=`, `/{owner}` | Home feed, repository search, profile with pinned repositories and a contribution graph |
| `/{owner}/{repo}` | Code: tabs, branch selector, latest commit, file table with each path's last commit, README, About column with a languages bar |
| `/{owner}/{repo}/tree/{branch}/{path}`, `/blob/{branch}/{path}` | Folders and files (line numbers, Raw/Blame); `/blob/{path}` resolves on the default branch |
| `/{owner}/{repo}/commits/{branch}`, `/commit/{sha}`, `/branches` | History by day, one commit with its unified diff, branches with ahead/behind counts |
| `/{owner}/{repo}/issues[?state=closed]`, `/issues/{n}`, `/issues/new` | Issue list with label chips, an issue's timeline and sidebar, the new-issue form |
| `/{owner}/{repo}/pulls`, `/pull/{n}[/commits\|/files]`, `/compare` | Pull requests; conversation with reviews and the merge box, commits, files changed as a diff |
| `/{owner}/{repo}/stargazers`, `/gists`, `/gist/{id}` | Stargazers, gist index, one gist |

POST `star`, `issues`, `pulls`, `issues/{n}/comments|state`, `pull/{n}/comments|state|reviews|merge`
mutate and land on the page they changed; prefix a route with `/api/` for JSON.
Ticks on this site are hours from Saturday 1 August 2026; the world clock (microseconds)
only advances "now" past the newest seeded event.
