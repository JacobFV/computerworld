# Create a world

## A desktop for one agent, ready to copy

If what you want is *one machine an agent can use* — a desktop, a browser, the
everyday applications and a couple of documents — copy
[`examples/worlds/agent-desktop.json`](../examples/worlds/agent-desktop.json) instead
of writing a world from scratch. It is one Ubuntu workstation for the user `ada`, with
Terminal, Files, Text Editor, Firefox, Calculator, Calendar, Notes, Image Viewer,
LibreOffice Calc and Visual Studio Code installed, three seeded documents under
`~/Documents`, and an intranet wiki at `http://wiki.internal/` for the browser to
reach. The gateway is closed to the internet and to the host, as every world's is.

The grants an actor needs are in the file, under `metadata.actor_session`, so the
session is a copy rather than a guess:

```rust
let definition = computerworld::WorldDefinition::from_json(include_str!(
    "../examples/worlds/agent-desktop.json"
))?;
let config: computerworld::EnvironmentConfig =
    serde_json::from_value(definition.metadata["actor_session"].clone())?;
let mut world = computerworld::World::new(definition, 7)?;
let session = world.environment(config)?;
let mut agent = world.actor(&session)?; // hand only this to the agent
```

`metadata.desktop_themes` is what gives the machine a shell to draw; without it there
are windows but no desktop around them. `installed_apps` is what makes an application
launchable — an id that is not there is `not_found` with the reason
`application_not_installed`, and `application.v1 list` shows the whole catalogue with
the reason each unlaunchable entry carries.
`crates/computerworld/tests/agent_desktop.rs` drives this world through an actor
session only, so it cannot rot.

## Anything else

Start with [`examples/custom-world`](../examples/custom-world), or copy the
structure of [`worlds/company-2026/world.json`](../worlds/company-2026/world.json)
and replace its fixtures. The kernel has no required company domain or machine.

1. Define OS profiles and each machine's address, user and initial files.
2. Define service nodes and explicit links from clients to those nodes.
3. Place service instances with registered kinds and state. Give them domains.
4. Declare DNS records and gateway restrictions as needed.
5. Parse using `WorldDefinition::from_json`, construct `World`, and create an
   environment granting only the intended machines and action families.
6. Test behavior from actor actions: resolving, navigating, mutating and reading
   from a second machine. Owner inspection is diagnostic evidence, not the actor's solution.

Keep secret evaluation answers outside the blueprint's actor-visible files and
service responses. Use deterministic context values for generated fixtures.
Store the definition, engine version, module versions, seed and action sequence
when sharing a reproducible episode.
