# Define an application

Run `cargo run -p computerworld --example custom-app`.

The counter keeps semantic state in a serializable value. An `activate` event
changes that state; its pure `page` method describes a text label and button using
the native page contract. No HTML, CSS, DOM, host timer or browser process is
involved. The same projection feeds structured observations and scene rendering.

The executable registers the module in a `Registry`, passes it to
`World::with_registry`, launches it through `application.v1`, obtains the button
geometry from the scene, and sends a pointer click. The resulting actor
observation reads `Count: 1`; no privileged state inspection supplies the answer.
