# Create a service

The runnable [`examples/custom-service`](../examples/custom-service) is the starting
point. Implement `Service` in your own crate; the kernel need not change.

Choose a unique versioned kind, validate initialization, and define HTTP routes.
Keep durable data in the supplied instance state. Use context identity for access
checks; do not trust an actor name supplied in the JSON request body. Return
native pages for interactive views and JSON for tools over the same state.

Register the handler before constructing the runtime, add a `ServiceDefinition`
with its kind/node/domain, and connect a client node. Verify a mutation by client
A is observable to client B through a fresh HTTP request. Add denial, invalid
payload and snapshot/restore tests. A view should represent received content;
changing service state must not silently update an already loaded page.

The SDK is described in [service-sdk.md](service-sdk.md).

## Serving HTML

A service may answer with `text/html` instead of a native page: the browser renders
it through the `cw-web` engine, fetching the `<link rel=stylesheet>` sheets and the
pictures the document references through the same synthetic network, and every
browser action (`click`, `fill`, `key`, `submit`, scrolling) works on the result.
`text/plain` is shown as preformatted text and `image/*` responses on their own.
For a site that is just a directory of authored files, the `static-site` service's
`files` map serves `.html`, `.css`, `.js` and pictures with their media types; see
[services/static-site/README.md](../services/static-site/README.md).

## HTML services

New services answer with HTML rather than `Page` JSON. Build pages with
`cw_service_common::html` (`el`, `link`, `form`, `text_input`, `button`, `Document`,
`HtmlResponse`), keep the stylesheet in a `.css` file next to the source and include
it with `include_str!`, give every link, input and button an `id` (the agent API), and
run every page through `html::validate_strict` in the service's tests, with
`cw-service-common = { path = "..", features = ["validate"] }` under
`[dev-dependencies]`. [service-sdk.md](service-sdk.md#html-services) describes the
layer; [html-migration.md](html-migration.md) is the recipe for moving an existing
`Page` service over, with the search service as the worked example.
