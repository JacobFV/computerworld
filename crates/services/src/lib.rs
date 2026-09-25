//! Optional bundled service registry. The kernel does not depend on this crate.
pub use cw_service_assistant as assistant;
pub use cw_service_bank as bank;
pub use cw_service_calendar as calendar;
pub use cw_service_chat as chat;
pub use cw_service_discord as discord;
pub use cw_service_docs as docs;
pub use cw_service_drive as drive;
pub use cw_service_forum as forum;
pub use cw_service_geo as geo;
pub use cw_service_git as git;
pub use cw_service_issues as issues;
pub use cw_service_mail as mail;
pub use cw_service_media as media;
pub use cw_service_messages as messages;
pub use cw_service_press as press;
pub use cw_service_search as search;
pub use cw_service_shop as shop;
pub use cw_service_slack as slack;
pub use cw_service_social as social;
pub use cw_service_speaker as speaker;
pub use cw_service_static_site as static_site;
pub use cw_service_wiki as wiki;
/// The simulated public web's kinds; the content packages bind sites to these. Slack and
/// Discord are public sites in their own right, each with its own semantics.
pub const WEB_KINDS: [&str; 13] = [
    "assistant",
    "bank",
    "discord",
    "drive",
    "forum",
    "geo",
    "media",
    "press",
    "search",
    "shop",
    "slack",
    "social",
    "wiki",
];
/// The company's internal messaging kinds: plain chat, and texting between handles.
pub const MESSAGING_KINDS: [&str; 2] = ["chat", "messages"];
/// Existing kinds that gained an additive `skin`; each crate owns its own skin vocabulary.
pub const SKINNED_KINDS: [&str; 6] = ["calendar", "chat", "docs", "git", "issues", "mail"];
/// Web kinds whose default `skin` is `plain`, i.e. the unthemed original rendering. A
/// themed page is what a *skinned* instance owes; plain deliberately has no theme.
pub const PLAIN_DEFAULT_KINDS: [&str; 1] = ["drive"];
pub fn register(registry: &mut cw_sdk::Registry) -> cw_protocol::Result<()> {
    git::register(registry)?;
    issues::register(registry)?;
    static_site::register(registry)?;
    mail::register(registry)?;
    chat::register(registry)?;
    messages::register(registry)?;
    slack::register(registry)?;
    discord::register(registry)?;
    docs::register(registry)?;
    calendar::register(registry)?;
    assistant::register(registry)?;
    bank::register(registry)?;
    drive::register(registry)?;
    forum::register(registry)?;
    geo::register(registry)?;
    media::register(registry)?;
    press::register(registry)?;
    search::register(registry)?;
    shop::register(registry)?;
    speaker::register(registry)?;
    social::register(registry)?;
    wiki::register(registry)?;
    #[cfg(feature = "oss-web")]
    cw_oss_web::register(registry)?;
    Ok(())
}
pub fn registry() -> cw_protocol::Result<cw_sdk::Registry> {
    let mut registry = cw_sdk::Registry::new();
    register(&mut registry)?;
    Ok(registry)
}
#[cfg(test)]
mod tests {
    fn context() -> cw_sdk::ServiceContext {
        cw_sdk::ServiceContext {
            actor: "alice".into(),
            source: "alice-mac".into(),
            tick: 0,
            seed: 1,
            instance: "site".into(),
        }
    }
    #[test]
    fn registry_has_independent_optional_services() {
        let r = super::registry().unwrap();
        assert_eq!(
            r.service_kinds().count(),
            22 + usize::from(cfg!(feature = "oss-web"))
        );
        assert!(r.service("speaker").is_ok());
        for kind in super::MESSAGING_KINDS {
            assert!(r.service(kind).is_ok(), "{kind} must be registered");
        }
        assert!(r.service("git").is_ok());
        assert!(r.service("static-site").is_ok());
        for kind in super::WEB_KINDS {
            assert!(r.service(kind).is_ok(), "{kind} must be registered");
        }
    }
    /// Every web kind serves a valid themed page at `/` from empty seed state, so a site can be
    /// bound to a node and browsed before the package that owns its content has landed.
    #[test]
    fn web_kinds_serve_a_valid_page_from_empty_state() {
        let r = super::registry().unwrap();
        for kind in super::WEB_KINDS {
            let service = r.service(kind).unwrap();
            let mut state = service
                .initialize(serde_json::json!({}), &context())
                .unwrap();
            let response = service
                .handle(
                    &mut state,
                    &context(),
                    &cw_protocol::HttpRequest::get("http://site.example/"),
                )
                .unwrap();
            assert_eq!(response.status, 200, "{kind}");
            // A migrated kind answers with HTML (docs/html-migration.md): it must parse, carry a
            // title and pass the strict validator. The rest still serve a valid themed page.
            if response
                .header("content-type")
                .is_some_and(|t| t.starts_with("text/html"))
            {
                let html = String::from_utf8(response.body).unwrap();
                cw_service_common::html::validate_strict(&html)
                    .unwrap_or_else(|e| panic!("{kind}: {e}"));
                let dom = cw_web::html::parse(&html);
                let title = dom
                    .descendants(cw_web::dom::Document::ROOT)
                    .find(|n| dom.is(*n, "title"))
                    .map(|n| dom.text_content(n))
                    .unwrap_or_default();
                assert!(!title.trim().is_empty(), "{kind} serves an untitled page");
                continue;
            }
            let page: cw_protocol::Page = serde_json::from_slice(&response.body).unwrap();
            page.validate().unwrap_or_else(|e| panic!("{kind}: {e}"));
            // A kind with a `plain` skin serves the unthemed original by default; that is
            // the whole point of plain, so only a skinned instance owes a theme.
            assert!(
                page.theme.is_some() || super::PLAIN_DEFAULT_KINDS.contains(&kind),
                "{kind} serves an unthemed page but does not default to a plain skin"
            );
        }
    }
    /// Skins are additive: a default state carries no `skin` key, so worlds and checkpoints
    /// written before skins existed round-trip byte-identically, and a typo still fails loudly.
    #[test]
    fn plain_skin_is_invisible_and_unknown_skins_are_rejected() {
        let r = super::registry().unwrap();
        let ctx = context();
        for kind in super::SKINNED_KINDS {
            let service = r.service(kind).unwrap();
            let state = service.initialize(serde_json::json!({}), &ctx).unwrap();
            assert!(
                state.get("skin").is_none(),
                "{kind} serialises a default skin"
            );
            assert!(
                service
                    .initialize(serde_json::json!({"skin": "nonesuch"}), &ctx)
                    .is_err(),
                "{kind} accepts an unknown skin"
            );
        }
    }
}
