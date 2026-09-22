use super::*;

/// Serialized instance data and declared module version. Registry code is supplied
/// by the embedding runtime and is never deserialized from a snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInstance {
    pub kind: String,
    pub version: u32,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RegisteredApplications {
    pub instances: BTreeMap<String, AppInstance>,
}
impl RegisteredApplications {
    pub fn launch(
        &mut self,
        registry: &cw_sdk::Registry,
        kind: &str,
        id: &str,
        initial: serde_json::Value,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<()> {
        if id.is_empty() || self.instances.contains_key(id) {
            return Err(cw_protocol::SimError::invalid(
                "empty or duplicate app instance",
            ));
        }
        let app = registry.application(kind)?;
        let state = app.initialize(initial, context)?;
        app.page(&state, context)?.validate()?;
        self.instances.insert(
            id.into(),
            AppInstance {
                kind: kind.into(),
                version: app.version(),
                state,
            },
        );
        Ok(())
    }
    pub fn event(
        &mut self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
        event: &cw_sdk::AppEvent,
    ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
        let instance = self
            .instances
            .get_mut(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        // A plugin's failed event cannot leave a partially mutated app state.
        let mut next = instance.state.clone();
        let effects = app.event(&mut next, context, event)?;
        app.page(&next, context)?.validate()?;
        instance.state = next;
        Ok(effects)
    }
    pub fn page(
        &self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<cw_protocol::Page> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        let page = app.page(&instance.state, context)?;
        page.validate()?;
        Ok(page)
    }
}
