# Synthetic browser

`BrowserState` is serialized browser state, independent of any service implementation.
`navigate`, `click`, `submit`, `reload`, and `key` receive an explicit synchronous
`FnMut(HttpRequest) -> Result<HttpResponse>` transport. The environment supplies the
canonical runtime's DNS/network/service path. History traversal and rendering do
not make requests. Cookies and storage are origin scoped; cookie Domain attributes
are rejected, and no host transport is implied.

Native page responses use `application/vnd.computerworld.page+json`. Every received
page is schema validated. Forms URL-encode controls within their own form. Explicit
fields can reference a control with `$control-id`; this allows a service to namespace
control IDs while retaining its request field names. Plain text, JSON, and HTML
outside the native media type display as text; no JavaScript executes.

## Image assets

Native image elements resolve their `source` relative to the received page URL.
Images use the same synthetic transport, with a strict same-origin policy that also
applies to redirects. The supported wire format is:

```text
Content-Type: application/vnd.computerworld.rgba+json
{"width":2,"height":1,"rgba":[255,0,0,255,0,0,255,255]}
```

The format is row-major RGBA8 with straight alpha. Decoding validates nonzero sizes,
exact byte count, and a 4 MiB pixel budget per image. The browser's 16 MiB decoded
cache evicts URLs in lexical order for deterministic behavior. Navigation can reuse
the cache; reload refreshes image requests. History entries retain shared assets,
so eviction or later content mutation never changes their rendered frame. Portable
checkpoints include cached and historical assets. `validate_assets()` validates
imported cache data and historical pages.

Failed/blocked assets retain alt text and put an error code in the history entry's
`image_errors`; they do not turn an otherwise valid document into a navigation
failure. Unsupported formats are explicitly reported as `image_format`.

`scene()` emits image primitives and semantic alt labels. It never fetches assets,
decodes responses, consults service backing state, or rasterizes. Numeric primitive
IDs derive deterministically from page element IDs to preserve identity across
content insertions. The separate renderer handles clipping, scaling, and pixels.
