# Issues and pull requests

Optional kind `issues`, registered with `cw_service_issues::register`.
State: `{"projects":{"OPS":{"name":"Operations","issues":{},"writers":["alice"]}}}`.
Empty ACL lists are public. IDs increase monotonically independently per project.

| API path | Methods | Body |
|---|---|---|
| `/api/search` | GET | — (query `q`, `assignee`) |
| `/api/projects` | GET | — |
| `/api/projects/{project}` | GET | — |
| `/api/projects/{project}/issues` | GET, POST | `title`, optional `body`, `assignee`, `kind` |
| `/api/projects/{project}/issues/{id}` | GET, POST, PATCH | Optional `status`, `title`, `body`, `assignee`, `labels` |
| `/api/projects/{project}/issues/{id}/comments` | POST | `body` |
| `/api/projects/{project}/issues/{id}/reviews` | POST | `decision`, optional `body` |

Kinds: `issue`, `pull_request`. Statuses: `open`, `in_progress`, `blocked`,
`closed`. Review decisions: `approve`, `request_changes`, `comment`. Reviews are
append-only and **do not overwrite PR open/closed state**. Reviews require PR kind.
This package does not pretend approval merges a Git commit; Git refs change only
through the git service's validated push transaction.

`/search` is a read-only view over the issues that already exist: it answers the
sidebar's search box and its "My issues" row, matching `q` against an issue's key,
title, body, labels, assignee and author, and `assignee` exactly. It holds no state.

The corresponding paths without `/api` supply native pages with forms. JSON and
URL-encoded forms mutate the same state. Comments and reviews use context actor
and logical tick. Wrong method is 405; missing record 404; malformed body 400;
ACL denial 403; invalid field 422. Rejected changes do not partially apply.

Seed issue records may omit optional fields; required fields are `id` and `title`.
