//! Renders the three sites' main pages (and one article page each) through the engine to
//! `research/site-stills/`. Ignored by default: pictures for the record, not a gate.
//! `cargo test -p cw-service-wiki --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_wiki::WikiService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

fn render(html: &str, path: &str, viewport: Viewport) {
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            sheets.push(parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Strict).unwrap());
        }
    }
    let media = Media::with_size(viewport.width as i32, viewport.height as i32);
    let styles = cw_web::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Strict).unwrap();
    let tree = cw_web::layout::layout(&doc, &styles, viewport);
    let scene = cw_web::paint::paint(&doc, &styles, &tree, viewport, &cw_web::paint::PaintContext::default());
    let frame = cw_render::Renderer::new().render(&scene);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&frame.rgba).unwrap();
    }
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/site-stills").join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn wikipedia_imdb_and_archive_stills() {
    let sites = [
        ("wikipedia", include_str!("../../../worlds/company-2026/sites/wikipedia.json"), "wikipedia.org", "Deterministic_simulation"),
        ("imdb", include_str!("../../../worlds/company-2026/sites/imdb.json"), "imdb.com", "Northbound_Signal"),
        ("archive", include_str!("../../../worlds/company-2026/sites/archive.json"), "archive.org", "Cavern_Runner_98"),
    ];
    let viewport = Viewport { width: 1280, height: 800, scale: 1, zoom: 100 };
    for (name, seed, host, article) in sites {
        let site: Value = serde_json::from_str(seed).unwrap();
        let ctx = ServiceContext { actor: "alice".into(), source: "alice-mac".into(), tick: 1, seed: 1, instance: name.into() };
        let mut state = WikiService.initialize(site["initial_state"].clone(), &ctx).unwrap();
        // The main page is the still of record; the article page is where the skin earns its name.
        let shots = [
            (format!("http://{host}/"), format!("{name}.png")),
            (format!("http://{host}/wiki/{article}"), format!("{name}-article.png")),
        ];
        for (url, file) in shots {
            let response = WikiService.handle(&mut state, &ctx, &HttpRequest::get(&url)).unwrap();
            assert_eq!(response.status, 200);
            render(&String::from_utf8(response.body).unwrap(), &file, viewport);
        }
    }
}
