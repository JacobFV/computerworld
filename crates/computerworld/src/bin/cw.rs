//! Persistent JSON-lines owner transport for integration/debugging.
//! Do not expose this privileged endpoint to an untrusted acting agent.
use computerworld::{reference_world, SimError, World};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// mimalloc rather than the system allocator: the engine makes and frees small
/// objects at a high rate, and the determinism corpus runs 15-23% faster on it
/// with the same hashes.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
fn main() {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout());
    let mut world: Option<World> = None;
    for line in stdin.lock().lines() {
        let result = match line {
            Ok(line) => serde_json::from_str::<Value>(&line)
                .map_err(SimError::from)
                .and_then(|v| dispatch(&mut world, v)),
            Err(e) => Err(SimError::new("transport", e.to_string())),
        };
        let output = match result {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(e) => json!({"ok":false,"error":e}),
        };
        if serde_json::to_writer(&mut stdout, &output).is_err()
            || writeln!(&mut stdout).is_err()
            || stdout.flush().is_err()
        {
            break;
        }
    }
}
fn required<'a>(v: &'a Value, k: &str) -> computerworld::Result<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| SimError::invalid(format!("missing {k}")))
}
fn dispatch(slot: &mut Option<World>, v: Value) -> computerworld::Result<Value> {
    let op = required(&v, "op")?;
    if op == "create" {
        let definition = match v.get("definition") {
            Some(d) => serde_json::from_value(d.clone())?,
            None => reference_world(),
        };
        let seed = v.get("seed").and_then(Value::as_u64).unwrap_or(0);
        let world = World::new(definition, seed)?;
        let value = json!({"world":world.definition().id,"state_hash":world.state_hash()?});
        *slot = Some(world);
        return Ok(value);
    }
    let world = slot
        .as_mut()
        .ok_or_else(|| SimError::invalid("create a world first"))?;
    Ok(match op {
        "environment" => {
            json!({"session":world.environment(serde_json::from_value(v.get("config").cloned().ok_or_else(||SimError::invalid("missing config"))?)?)?})
        }
        "step" => serde_json::to_value(world.step(
            required(&v, "session")?,
            serde_json::from_value(v.get("actions").cloned().unwrap_or(json!([])))?,
        )?)?,
        "observe" => serde_json::to_value(world.observe(required(&v, "session")?)?)?,
        "scene" => serde_json::to_value(world.scene(
            required(&v, "session")?,
            dimension(&v, "width", 800)?,
            dimension(&v, "height", 600)?,
        )?)?,
        #[cfg(feature = "render")]
        "render" => {
            use sha2::{Digest, Sha256};
            let f = world.render(
                required(&v, "session")?,
                dimension(&v, "width", 800)?,
                dimension(&v, "height", 600)?,
            )?;
            json!({"width":f.width,"height":f.height,"sha256":format!("{:x}",Sha256::digest(&f.rgba)),"bytes":f.rgba.len()})
        }
        "export_snapshot" => json!({"snapshot":world.export_snapshot()?}),
        "import_snapshot" => {
            world.import_snapshot(required(&v, "snapshot")?)?;
            json!({"state_hash":world.state_hash()?})
        }
        "reset" => {
            world.reset(v.get("seed").and_then(Value::as_u64).unwrap_or(0))?;
            json!({"state_hash":world.state_hash()?})
        }
        "trajectory" => serde_json::to_value(world.trajectory())?,
        "inspect" => world.inspect(),
        "state_hash" => json!({"state_hash":world.state_hash()?}),
        _ => return Err(SimError::invalid("unknown owner transport operation")),
    })
}
fn dimension(v: &Value, key: &str, default: u32) -> computerworld::Result<u32> {
    match v.get(key) {
        None => Ok(default),
        Some(n) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| SimError::invalid("invalid viewport dimension")),
    }
}
