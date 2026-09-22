//! A pure application plugin: event state and page projection are separate.
use cw_protocol::{Page, PageAction, PageElement, Result};
use cw_sdk::{AppContext, AppEffect, AppEvent, Application};
use serde_json::{json, Value};
use std::collections::BTreeMap;
struct Counter;
impl Application for Counter {
    fn kind(&self) -> &str {
        "example.counter"
    }
    fn initialize(&self, _: Value, _: &AppContext) -> Result<Value> {
        Ok(json!({"count":0}))
    }
    fn event(&self, state: &mut Value, _: &AppContext, event: &AppEvent) -> Result<Vec<AppEffect>> {
        if matches!(event.kind.as_str(), "activate" | "click")
            && event.target.as_deref() == Some("increment")
        {
            state["count"] = json!(state["count"]
                .as_u64()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| cw_protocol::SimError::invalid("counter overflow"))?);
        }
        Ok(vec![])
    }
    fn page(&self, state: &Value, _: &AppContext) -> Result<Page> {
        let mut page = Page::new("Custom counter");
        page.elements.push(PageElement::Text {
            id: "value".into(),
            text: format!("Count: {}", state["count"]),
        });
        page.elements.push(PageElement::Button {
            id: "increment".into(),
            text: "Increment".into(),
            action: PageAction {
                method: "EVENT".into(),
                url: "increment".into(),
                fields: BTreeMap::new(),
            },
            style: None,
        });
        Ok(page)
    }
}
fn main() -> Result<()> {
    let mut definition =
        cw_protocol::WorldDefinition::from_json(include_str!("../../worlds/unrelated-lab/world.json"))?;
    definition.computers[0]
        .installed_apps
        .push("example.counter".into());
    let mut registry = cw_sdk::Registry::new();
    registry.register_application(Counter)?;
    let mut world = computerworld::World::with_registry(definition, 7, registry)?;
    let actor = world.environment(cw_protocol::EnvironmentConfig::desktop("researcher", "lab"))?;
    let launched = world.step(
        &actor,
        vec![cw_protocol::ActionEnvelope::new(
            "application.v1",
            "launch",
            "lab",
            json!({"kind":"example.counter","instance":"counter"}),
        )],
    )?;
    assert!(launched.outcomes[0].success);
    let scene = world.scene(&actor, 640, 480)?;
    let button = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some("increment"))
        .ok_or_else(|| cw_protocol::SimError::invalid("missing button"))?;
    let (x, y) = button
        .transform
        .point(button.bounds.x + 3, button.bounds.y + 3);
    let result = world.step(
        &actor,
        vec![cw_protocol::ActionEnvelope::new(
            "pointer.v1",
            "click",
            "lab",
            json!({"x":x,"y":y,"width":640,"height":480}),
        )],
    )?;
    assert!(result.outcomes[0].success);
    let observation = serde_json::to_string_pretty(&world.observe(&actor)?)?;
    assert!(observation.contains("Count: 1"));
    println!("{observation}");
    Ok(())
}
