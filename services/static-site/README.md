# Native synthetic site

Optional kind `static-site`, registered with `cw_service_static_site::register`.

```json
{"pages":{"/":{"version":1,"title":"Hello","elements":[{"kind":"text","id":"intro","text":"A synthetic site"}]}},"records":{}}
```

Exact page paths respond to GET with the versioned native Page media type.
`GET /api/records` lists shared data. `/api/records/{key}` supports GET,
PUT/POST (create or replace arbitrary JSON), DELETE. `/records` renders those same
records as a native page, allowing browser clients to observe API mutations.
`readers` and `writers` optional actor ACL arrays default to public access.
Missing pages/records are 404; forbidden actions 403; wrong methods 405;
malformed mutation JSON 400. Creates return 201, replacements 200, deletes 204.
No real HTTP/HTML execution, host networking or external resources occur here.

Optional `assets` maps exact paths to `{content_type, json}` or
`{content_type, bytes:[u8...]}`. For native browser images use
`{"content_type":"application/vnd.computerworld.rgba+json","json":{"width":1,"height":1,"rgba":[255,0,0,255]}}`.
The browser obtains these through the same synthetic DNS/network/service path;
assets require the site's read ACL and never access the host filesystem.
