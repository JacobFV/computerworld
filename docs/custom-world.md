# Create a world

## A desktop for one agent, ready to copy

If what you want is *one machine an agent can use* — a desktop, a browser, the
everyday applications and a couple of documents — copy
[`worlds/agent-desktop/world.json`](../worlds/agent-desktop/world.json) instead
of writing a world from scratch. It is one Ubuntu workstation for the user `ada`, with
Terminal, Files, Text Editor, Firefox, Calculator, Calendar, Notes, Image Viewer,
LibreOffice Calc and Visual Studio Code installed, three seeded documents under
`~/Documents`, and an intranet wiki at `http://wiki.internal/` for the browser to
reach. The gateway is closed to the internet and to the host, as every world's is.

The grants an actor needs are in the file, under `metadata.actor_session`, so the
session is a copy rather than a guess:

```rust
let definition = computerworld::WorldDefinition::from_json(include_str!(
    "../worlds/agent-desktop/world.json"
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

Start with [`worlds/unrelated-lab`](../worlds/unrelated-lab), or copy the
structure of [`worlds/company-2026/world.json`](../worlds/company-2026/world.json)
and replace its fixtures. The kernel has no required company domain or machine.

Past a few machines, write a [blueprint](blueprint.md) rather than the definition
itself: it is the same schema in YAML, with the files a machine starts with kept as
real files in a directory, services split one to a file, each service's node, link
and DNS records derived from where it sits, and the environment it reads declared
up front. `cw-world build` resolves one into an ordinary world definition;
[`worlds/agent-desktop/world.yml`](../worlds/agent-desktop/world.yml)
is the world above, written that way.

1. Define OS profiles and each machine's address, user and initial files.
2. Define service nodes and explicit links from clients to those nodes.
3. Place service instances with registered kinds and state. Give them domains.
4. Declare DNS records and gateway restrictions as needed. The public web is not
   yours to declare: every world joins the [built-in internet](../worlds/internet)
   unless it sets `internet: false`, and a machine reaches it when its gateway's
   `allow_internet` says so. Name `edge-router` to choose the uplink yourself, or
   leave it and each machine gets one.
5. Parse using `WorldDefinition::from_json` (or resolve a blueprint with
   `cw-world build`), construct `World`, and create an
   environment granting only the intended machines and action families.
6. Test behavior from actor actions: resolving, navigating, mutating and reading
   from a second machine. Owner inspection is diagnostic evidence, not the actor's solution.

Keep secret evaluation answers outside the blueprint's actor-visible files and
service responses. Use deterministic context values for generated fixtures.
Store the definition, engine version, module versions, seed and action sequence
when sharing a reproducible episode.

The fields each of these steps fills in are listed in the
[world schema](world-schema.md). A machine's insides are
[computers](computers.md) and its screen is the [desktop](desktop-gui.md); the wiring
between machines is [networking](networking.md). To add something the engine does not
already have, write an [application](custom-application.md) or a
[service](custom-service.md). Once the world runs, the
[agent API](agent-api.md) is how an actor is let into it.
