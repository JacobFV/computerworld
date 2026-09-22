use super::*;

impl Environment {
    pub(super) fn project_page(
        &self,
        actor: &str,
        machine: &str,
        state: &MachineSession,
    ) -> Result<Page> {
        if let Some(instance) = &state.active_app {
            state
                .registered
                .page(
                    &self.app_registry,
                    instance,
                    &self.app_context(actor, machine, instance),
                )
                .map_err(actor_error)
        } else {
            Ok(active_page(state))
        }
    }
    pub(super) fn app_context(
        &self,
        actor: &str,
        machine: &str,
        instance: &str,
    ) -> cw_sdk::AppContext {
        cw_sdk::AppContext {
            actor: actor.into(),
            machine: machine.into(),
            instance: instance.into(),
            tick: self.runtime.tick(),
            seed: self.runtime.seed(),
        }
    }
    pub(super) fn custom_launch(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        p: &Value,
    ) -> Result<Value> {
        let kind = string(p, "kind")?;
        let instance = p.get("instance").and_then(Value::as_str).unwrap_or(kind);
        let ctx = self.app_context(actor, machine, instance);
        let registry = self.app_registry.clone();
        let m = self.machine_mut(id, machine)?;
        m.registered.launch(
            &registry,
            kind,
            instance,
            p.get("initial").cloned().unwrap_or(Value::Null),
            &ctx,
        )?;
        m.custom_page = Some(m.registered.page(&registry, instance, &ctx)?);
        m.active_app = Some(instance.into());
        m.browser_visible = false;
        m.focused_input = None;
        Ok(json!({"instance":instance}))
    }
    pub(super) fn custom_event(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        p: &Value,
    ) -> Result<Value> {
        let instance = p
            .get("instance")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                self.sessions
                    .get(id)?
                    .machines
                    .get(machine)?
                    .active_app
                    .clone()
            })
            .ok_or_else(|| SimError::invalid("application instance required"))?;
        let event: cw_sdk::AppEvent =
            serde_json::from_value(p.get("event").cloned().unwrap_or_else(|| p.clone()))?;
        let registry = self.app_registry.clone();
        let ctx = self.app_context(actor, machine, &instance);
        let effects = self
            .machine_mut(id, machine)?
            .registered
            .event(&registry, &instance, &ctx, &event)?;
        self.custom_effects(id, machine, actor, &instance, effects)?;
        let ctx = self.app_context(actor, machine, &instance);
        let m = self.machine_mut(id, machine)?;
        m.custom_page = Some(m.registered.page(&registry, &instance, &ctx)?);
        Ok(serde_json::to_value(&m.custom_page)?)
    }
    pub(super) fn custom_effects(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        instance: &str,
        effects: Vec<cw_sdk::AppEffect>,
    ) -> Result<()> {
        let mut pending: std::collections::VecDeque<_> = effects.into();
        let mut count = 0;
        while let Some(effect) = pending.pop_front() {
            count += 1;
            if count > 1024 {
                return Err(SimError::invalid("application effect budget exceeded"));
            }
            let response = match effect {
                cw_sdk::AppEffect::ReadFile { path } => Some(
                    json!({"operation":"read_file","path":path,"bytes":self.runtime.read_file(machine,&path)?}),
                ),
                cw_sdk::AppEffect::WriteFile { path, bytes } => {
                    self.runtime.write_file(machine, actor, &path, &bytes)?;
                    Some(json!({"operation":"write_file","path":path}))
                }
                cw_sdk::AppEffect::Http { request } => Some(
                    json!({"operation":"http","response":self.runtime.http(machine,actor,request)?}),
                ),
                cw_sdk::AppEffect::Emit { name, data } => {
                    self.runtime
                        .record_event(&name, Some(machine), Some(actor), data);
                    None
                }
                cw_sdk::AppEffect::Launch { application } => {
                    let action = ActionEnvelope::new(
                        "application.v1",
                        "launch",
                        machine,
                        json!({"kind":application}),
                    );
                    self.dispatch(id, actor, &action)?;
                    None
                }
            };
            if let Some(data) = response {
                let registry = self.app_registry.clone();
                let ctx = self.app_context(actor, machine, instance);
                let event = cw_sdk::AppEvent {
                    kind: "effect_result".into(),
                    target: None,
                    data,
                };
                pending.extend(
                    self.machine_mut(id, machine)?
                        .registered
                        .event(&registry, instance, &ctx, &event)?,
                );
            }
        }
        Ok(())
    }
}
