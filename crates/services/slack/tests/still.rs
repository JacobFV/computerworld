//! Renders the Slack workspace through the engine to `research/studies/site-stills/slack.png`
//! (and the thread view beside it). Ignored by default: a picture for the record, not a
//! gate. `cargo test -p cw-service-slack --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_slack::SlackService;
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
    for area in &scene.scrolls {
        println!(
            "scroll {} bounds {:?} offset {} extent {}",
            area.target, area.bounds, area.offset, area.extent
        );
    }
    let frame = cw_render::Renderer::new().render(&scene);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&frame.rgba).unwrap();
    }
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../research/studies/site-stills");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join(file);
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn slack_stills() {
    let raw = std::fs::read_to_string("../../../worlds/company-2026/sites/slack.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    let ctx = ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "slack".into(),
    };
    let mut state = SlackService
        .initialize(site["initial_state"].clone(), &ctx)
        .unwrap();
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (url, file) in [
        ("http://slack.com/channels/eng", "slack.png"),
        (
            "http://slack.com/channels/eng?thread=chat-1",
            "slack-thread.png",
        ),
        (
            "http://slack.com/channels/alice|bob?members=1",
            "slack-dm.png",
        ),
    ] {
        let response = SlackService
            .handle(&mut state, &ctx, &HttpRequest::get(url))
            .unwrap();
        assert_eq!(response.status, 200);
        render(&String::from_utf8(response.body).unwrap(), file, viewport);
    }
}

/// Renders the HTML file named by `STILL_HTML` to `STILL_PNG`: the tool behind the
/// minimal reproductions of what the engine draws differently.
#[test]
#[ignore]
fn repro_still() {
    let (Ok(html), Ok(png)) = (std::env::var("STILL_HTML"), std::env::var("STILL_PNG")) else {
        return;
    };
    let viewport = Viewport {
        width: 400,
        height: 200,
        scale: 1,
        zoom: 100,
    };
    render(&std::fs::read_to_string(html).unwrap(), &png, viewport);
}
