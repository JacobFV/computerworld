//! Renders the seeded Google Calendar week through the engine to
//! `research/site-stills/google-calendar.png`. Ignored by default: it is a picture for the
//! record, not a gate. `cargo test -p cw-service-calendar --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_calendar::{CalendarService, DAY_US, HOUR_US};
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

const WORLD: &str = include_str!("../../../worlds/company-2026/world.json");

fn render(html: &str, file: &str, viewport: Viewport) {
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
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/site-stills").join(file);
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn google_calendar_stills() {
    let world: Value = serde_json::from_str(WORLD).unwrap();
    let service = world["services"].as_array().unwrap().iter().find(|s| s["id"] == "google-calendar").unwrap();
    // Wednesday 23 September 2026, 10:30: the busiest seeded week, with the red line on screen.
    let ctx = ServiceContext { actor: "alice".into(), source: "alice-mac".into(), tick: 6 * DAY_US + HOUR_US + HOUR_US / 2, seed: 1, instance: "google-calendar".into() };
    let mut state = CalendarService.initialize(service["initial_state"].clone(), &ctx).unwrap();
    let viewport = Viewport { width: 1280, height: 800, scale: 1, zoom: 100 };
    for (url, file) in [
        ("http://calendar.google.com/", "google-calendar.png"),
        ("http://calendar.google.com/?day=6&event=design-crit-scene-graph", "google-calendar-event.png"),
        ("http://calendar.google.com/?view=month&day=6", "google-calendar-month.png"),
        ("http://calendar.google.com/?day=12", "google-calendar-allday.png"),
    ] {
        let response = CalendarService.handle(&mut state, &ctx, &HttpRequest::get(url)).unwrap();
        assert_eq!(response.status, 200);
        render(&String::from_utf8(response.body).unwrap(), file, viewport);
    }
}
