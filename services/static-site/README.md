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

Optional `files` maps absolute paths to authored files served with the media type
their extension implies (`.html`, `.css`, `.js`, `.png`, `.jpg`, `.gif`, `.svg`,
`.txt`, `.json`, ...), so a site can be an ordinary HTML directory:

```json
{"files":{
  "/index.html":"<!DOCTYPE html><html><head><link rel=stylesheet href=/site.css></head><body><h1>Hello</h1><img src=/logo.png></body></html>",
  "/site.css":"h1{color:#c00}",
  "/logo.png":{"bytes":[137,80,78,71,13,10,26,10]},
  "/notes.txt":{"text":"plain text","content_type":"text/plain"}
}}
```

A value is the file's text, or an object with `text` or `bytes` and an optional
`content_type` override. A directory URL (`/docs/`) serves its `index.html`; the
directory named without its slash (`/docs`) redirects to it. The browser renders
`text/html` through its engine, fetching the page's stylesheets and pictures through
the same synthetic network; exact `pages` and `assets` paths take precedence over
`files`.
