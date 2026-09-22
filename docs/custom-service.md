# Create a service

The runnable [`examples/custom-service`](../examples/custom-service) is the starting
point. Implement `Service` in your own crate; the kernel need not change.

Choose a unique versioned kind, validate initialization, and define HTTP routes.
Keep durable data in the supplied instance state. Use context identity for access
checks; do not trust an actor name supplied in the JSON request body. Return HTML for
interactive views and JSON for tools over the same state.

Register the handler before constructing the runtime, add a `ServiceDefinition`
with its kind/node/domain, and connect a client node. Verify a mutation by client
A is observable to client B through a fresh HTTP request. Add denial, invalid
payload and snapshot/restore tests. A view should represent received content;
changing service state must not silently update an already loaded page.

The SDK is described in [service-sdk.md](service-sdk.md).

## Answering with HTML

A service answers with `text/html`, and the browser renders it through the `cw-web`
engine: it fetches the `<link rel=stylesheet>` sheets and the pictures the document
references over the same synthetic network, runs the page's own scripts, and every
browser action (`click`, `fill`, `key`, `submit`, scrolling) works on the result.
`text/plain` is shown as preformatted text and `image/*` responses on their own.

Build pages with `cw_service_common::html` (`el`, `link`, `form`, `text_input`,
`button`, `Document`, `HtmlResponse`), keep the stylesheet in a `.css` file next to the
source and include it with `include_str!`, give every link, input and button an `id`
(the agent API addresses elements by id), and run every page through
`html::validate_strict` in the service's tests, with
`cw-service-common = { path = "..", features = ["validate"] }` under
`[dev-dependencies]`. [service-sdk.md](service-sdk.md#html-services) describes the
layer.

For a site that is just a directory of authored files, the `static-site` service's
`files` map serves `.html`, `.css`, `.js` and pictures with their media types; see
[crates/services/static-site/README.md](../crates/services/static-site/README.md).

## Moving an old service over

A service written before 0.2.0 returned `Page` JSON. That still renders — the browser
converts it — but a new one should not use it, and an existing one is worth moving:
[html-migration.md](html-migration.md) is the recipe, with the search service as the
worked example, and it says how to keep the element ids an agent's script depends on.
