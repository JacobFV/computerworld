//! Renders the Google home page through the engine to `research/studies/google-ceiling/engine.png`,
//! the companion of `mock.html` (Chromium) in that directory. Ignored by default: it is a
//! picture for the record, not a gate. `cargo test -p cw-service-search --test still -- --ignored`.
use cw_protocol::{HttpRequest, ServiceDefinition};
use cw_sdk::{Service, ServiceContext};
use cw_service_search::SearchService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

/// The reference company and the internet it joins: the engine indexes both.
const WORLDS: [&str; 2] = [
    include_str!("../../../../worlds/company-2026/world.json"),
    include_str!("../../../../worlds/internet/world.json"),
];

fn render(html: &str, path: &str, viewport: Viewport) {
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            sheets.push(
                parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Strict)
                    .unwrap(),
            );
        }
    }
    let media = Media::with_size(viewport.width as i32, viewport.height as i32);
    let styles = cw_web::style::cascade(
        &doc,
        &sheets,
        &media,
        &MatchContext::new(),
        Strictness::Strict,
    )
    .unwrap();
    let tree = cw_web::layout::layout(&doc, &styles, viewport);
    let scene = cw_web::paint::paint(
        &doc,
        &styles,
        &tree,
        viewport,
        &cw_web::paint::PaintContext::default(),
    );
    let frame = cw_render::Renderer::new().render(&scene);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&frame.rgba).unwrap();
    }
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../research/studies/google-ceiling")
        .join(path);
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn google_home_and_results_stills() {
    let services: Vec<ServiceDefinition> = WORLDS
        .iter()
        .flat_map(|w| {
            let world: Value = serde_json::from_str(w).unwrap();
            serde_json::from_value::<Vec<ServiceDefinition>>(world["services"].clone()).unwrap()
        })
        .collect();
    let service = services.iter().find(|s| s.id == "google-search").unwrap();
    let ctx = ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "google-search".into(),
    };
    let mut state = SearchService
        .initialize_in(service.initial_state.clone(), &ctx, &services)
        .unwrap();
    // The still matches the mock: a first visit, no history of alice's under the box.
    state["history"] = Value::Object(Default::default());
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (url, file) in [
        ("http://google.com/", "engine.png"),
        ("http://google.com/search?q=atlas", "engine-results.png"),
    ] {
        let response = SearchService
            .handle(&mut state, &ctx, &HttpRequest::get(url))
            .unwrap();
        assert_eq!(response.status, 200);
        render(&String::from_utf8(response.body).unwrap(), file, viewport);
    }
}
