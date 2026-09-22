//! A page may name any icon in `cw_protocol::PAGE_ICONS`; each must be a symbol this
//! renderer bundles, or a validated page would paint a blank where its button is.
#[test]
fn every_page_icon_is_a_bundled_symbol() {
    for name in cw_protocol::PAGE_ICONS {
        let asset = format!("symbol/{name}");
        assert!(
            cw_render::SYMBOLS.iter().any(|(id, _)| *id == asset),
            "page icon {name} has no bundled symbol"
        );
    }
}
