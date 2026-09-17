# Register a service

Run `cargo run -p computerworld --example custom-service`.

The example registers `example.counter`, adds a service node and explicit network
link to the unrelated lab world, and reaches it using an actor HTTP action. The
service sees the source computer, actor and logical tick; it has no browser or OS
dependency. Its instance state is checkpointed by the canonical runtime.

`GET` reads the counter and `POST` increments it. Place additional instances under
different domains with independent initial state without changing the kernel.
The registry contains trusted Rust code; it is not an untrusted plugin sandbox.
