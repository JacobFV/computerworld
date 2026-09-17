//! Actor-only information transfer through a generic multi-computer world.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, HttpRequest, HttpResponse};
use serde_json::{json, Value};
fn act(
    world: &mut World,
    session: &str,
    machine: &str,
    family: &str,
    op: &str,
    payload: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let result = world.step(
        session,
        vec![ActionEnvelope::new(family, op, machine, payload)],
    )?;
    let out = &result.outcomes[0];
    if !out.success {
        return Err(format!("action failed: {:?}", out.error).into());
    }
    Ok(out.value.clone())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new(reference_world(), 7)?;
    let alice = world.environment(EnvironmentConfig::desktop("alice", "alice-mac"))?;
    let bob = world.environment(EnvironmentConfig::desktop("bob", "bob-windows"))?;
    let input = act(
        &mut world,
        &alice,
        "alice-mac",
        "filesystem.v1",
        "read",
        json!({"path":"launch.txt"}),
    )?;
    let derived = input["content"]
        .as_str()
        .ok_or("missing content")?
        .replace("Status: pending", "Status: reviewed");
    act(
        &mut world,
        &alice,
        "alice-mac",
        "filesystem.v1",
        "write",
        json!({"path":"status.txt","content":derived}),
    )?;
    act(
        &mut world,
        &alice,
        "alice-mac",
        "browser.v1",
        "navigate",
        json!({"url":"http://intranet.internal/"}),
    )?;
    let mail = act(
        &mut world,
        &alice,
        "alice-mac",
        "browser.v1",
        "click",
        json!({"id":"mail"}),
    )?;
    println!("Mail received over synthetic DNS/HTTP: {mail}");
    let request = HttpRequest::json(
        "PUT",
        "http://docs.internal/api/documents/launch",
        &json!({"body":"Atlas checklist reviewed using local launch.txt and mail.","revision":1}),
    )?;
    let updated: HttpResponse = serde_json::from_value(act(
        &mut world,
        &alice,
        "alice-mac",
        "http.v1",
        "request",
        serde_json::to_value(request)?,
    )?)?;
    if !(200..300).contains(&updated.status) {
        return Err(format!("document update returned {}", updated.status).into());
    }
    let request = HttpRequest::get("http://docs.internal/api/documents/launch");
    let seen: HttpResponse = serde_json::from_value(act(
        &mut world,
        &bob,
        "bob-windows",
        "http.v1",
        "request",
        serde_json::to_value(request)?,
    )?)?;
    assert!(String::from_utf8_lossy(&seen.body).contains("reviewed"));
    println!(
        "Bob independently reads shared document: {}",
        String::from_utf8_lossy(&seen.body)
    );
    for command in [
        "git clone http://git.internal/repos/onboarding onboarding",
        "cd onboarding",
        "echo reviewed > checklist.txt",
        "git add checklist.txt",
        "git commit -m reviewed",
        "git push origin main",
    ] {
        let out = act(
            &mut world,
            &alice,
            "alice-mac",
            "terminal.v1",
            "execute",
            json!({"command":command}),
        )?;
        if out["exit_code"] != 0 {
            return Err(format!("{command}: {}", out["stderr"]).into());
        }
    }
    for command in [
        "git clone http://git.internal/repos/onboarding onboarding",
        "cd onboarding",
        "cat checklist.txt",
    ] {
        let out = act(
            &mut world,
            &bob,
            "bob-windows",
            "terminal.v1",
            "execute",
            json!({"command":command}),
        )?;
        if out["exit_code"] != 0 {
            return Err(format!("{command}: {}", out["stderr"]).into());
        }
        println!("Bob: {command}: {}", out["stdout"]);
    }
    let request = HttpRequest::json(
        "POST",
        "http://chat.internal/api/channels/general/messages",
        &json!({"text":"Atlas checklist reviewed; source update pushed."}),
    )?;
    let response: HttpResponse = serde_json::from_value(act(
        &mut world,
        &alice,
        "alice-mac",
        "http.v1",
        "request",
        serde_json::to_value(request)?,
    )?)?;
    assert_eq!(response.status, 200);
    act(
        &mut world,
        &bob,
        "bob-windows",
        "browser.v1",
        "navigate",
        json!({"url":"http://chat.internal/channels/general"}),
    )?;
    assert!(serde_json::to_string(&world.observe(&bob)?)?.contains("source update pushed"));
    let checkpoint = world.snapshot();
    let branch = world.fork(&checkpoint)?;
    assert_eq!(world.state_hash()?, branch.state_hash()?);
    println!(
        "{} causal events; checkpoint/fork hash {}",
        world.trajectory().len(),
        world.state_hash()?
    );
    Ok(())
}
