# Define a world

`world.json` is an unrelated one-machine laboratory. No company service or domain
is required. Run its actor workflow in `cargo test -p computerworld --test behaviors
unrelated_world_runs_without_reference_services`.

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
