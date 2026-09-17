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
