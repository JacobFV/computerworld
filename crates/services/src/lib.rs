//! Optional bundled service registry. The kernel does not depend on this crate.
pub use cw_service_calendar as calendar;
pub use cw_service_chat as chat;
pub use cw_service_docs as docs;
pub use cw_service_git as git;
pub use cw_service_issues as issues;
pub use cw_service_mail as mail;
pub use cw_service_static_site as static_site;
pub fn register(registry: &mut cw_sdk::Registry) -> cw_protocol::Result<()> {
    git::register(registry)?;
    issues::register(registry)?;
    static_site::register(registry)?;
    mail::register(registry)?;
    chat::register(registry)?;
    docs::register(registry)?;
    calendar::register(registry)?;
    Ok(())
}
pub fn registry() -> cw_protocol::Result<cw_sdk::Registry> {
    let mut registry = cw_sdk::Registry::new();
    register(&mut registry)?;
    Ok(registry)
}
#[cfg(test)]
mod tests {
    #[test]
    fn registry_has_independent_optional_services() {
        let r = super::registry().unwrap();
        assert_eq!(r.service_kinds().count(), 7);
        assert!(r.service("git").is_ok());
        assert!(r.service("static-site").is_ok());
    }
}
