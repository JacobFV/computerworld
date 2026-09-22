# Unrelated lab

One Linux terminal, one file, no network beyond itself. Nothing here comes from the
reference company: no `.internal` domain, no shared service, no seeded person. That is
its point — it is the world that proves the kernel holds no reference-world defaults,
and the smallest thing to copy when writing a world by hand.

`cargo test -p computerworld --test behaviors unrelated_world_runs_without_reference_services`
runs its actor workflow. The `custom-app` and `custom-service` examples both boot it, so
a new application or service can be shown working without a company around it.

```rust
let definition = computerworld::WorldDefinition::from_json(
    include_str!("world.json"),
)?;
let mut world = computerworld::World::new(definition, 9)?;
let actor = world.environment(
    computerworld::EnvironmentConfig::terminal("researcher", "lab"),
)?;
let result = world.step(&actor, vec![computerworld::ActionEnvelope::new(
    "terminal.v1", "execute", "lab", serde_json::json!({"command":"cat input.txt"}),
)])?;
```

Profiles control home paths, case sensitivity and shell dialect. Initial file
paths can be relative to the profile's home. Networks require declared links;
unknown public-looking names cannot escape to host DNS. Services reference node
IDs and registered module kinds, with their initial state supplied in the same
serialized definition.

`world.json` is written by hand — this world has no blueprint, because a blueprint
would be longer than the world.
