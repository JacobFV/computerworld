//! Renders each seeded discussion site's main page (and one thread page) through the engine
//! to `research/site-stills/<site>.png`. Ignored by default: pictures for the record, not a
//! gate. `cargo test -p cw-service-forum --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_forum::ForumService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

fn render(html: &str, file: &str, viewport: Viewport) {
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
    let target = if file.starts_with('/') {
        PathBuf::from(file)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../research/site-stills")
            .join(file)
    };
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn forum_site_stills() {
    let ctx = ServiceContext {
        actor: "bob".into(),
        source: "bob-linux".into(),
        tick: 60,
        seed: 1,
        instance: "forum".into(),
    };
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (site, host, thread) in [
        ("hackernews", "news.ycombinator.com", "/item?id=t-9001"),
        ("reddit", "reddit.com", "/r/programming/comments/t-5120"),
        ("stackoverflow", "stackoverflow.com", "/questions/t-4411"),
        ("quora", "quora.com", "/questions/t-3000"),
        ("yelp", "yelp.com", "/r/restaurants/comments/t-1000"),
        ("craigslist", "craigslist.org", "/item?id=t-7000"),
    ] {
        let path = format!(
            "{}/../../../worlds/company-2026/sites/{site}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut state = ForumService
            .initialize(file["initial_state"].clone(), &ctx)
            .unwrap();
        for (url, name) in [
            (format!("http://{host}/"), format!("{site}.png")),
            (
                format!("http://{host}{thread}"),
                format!("{site}-thread.png"),
            ),
        ] {
            let response = ForumService
                .handle(&mut state, &ctx, &HttpRequest::get(&url))
                .unwrap();
            assert_eq!(response.status, 200, "{url}");
            render(&String::from_utf8(response.body).unwrap(), &name, viewport);
        }
    }
}

/// Renders the HTML file named by `FORUM_SCRATCH` to the PNG beside it: a bench for
/// reducing an engine gap to a few lines. `FORUM_SCRATCH_SIZE=<w>x<h>` picks the viewport.
#[test]
#[ignore]
fn scratch() {
    let Ok(path) = std::env::var("FORUM_SCRATCH") else {
        return;
    };
    let html = std::fs::read_to_string(&path).unwrap();
    let size = std::env::var("FORUM_SCRATCH_SIZE").unwrap_or_default();
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((400, 200));
    render(
        &html,
        &format!("{path}.png"),
        Viewport {
            width: w,
            height: h,
            scale: 1,
            zoom: 100,
        },
    );
}
