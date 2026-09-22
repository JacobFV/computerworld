//! Renders each shipped speaker's page through the engine to `research/studies/site-stills/`.
//! Ignored by default: pictures for the record, not a gate.
//! `cargo test -p cw-service-speaker --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_speaker::SpeakerService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::layout::{LayoutCache, LayoutOptions, ScrollState};
use cw_web::paint::{ImageMap, PaintContext, RgbaImage};
use cw_web::{Strictness, Viewport};
use serde_json::{json, Value};
use std::path::PathBuf;

fn ctx(tick: u64) -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick,
        seed: 1,
        instance: "speaker".into(),
    }
}
fn render(state: &mut Value, host: &str, html: &str, file: &str, viewport: Viewport) {
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    let mut images = ImageMap::default();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            sheets.push(
                parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Strict)
                    .unwrap(),
            );
        }
        if doc.is(node, "img") {
            let src = doc.attr(node, "src").unwrap_or_default().to_owned();
            if images.0.contains_key(&src) {
                continue;
            }
            let reply = SpeakerService
                .handle(
                    state,
                    &ctx(1),
                    &HttpRequest::get(format!("http://{host}{src}")),
                )
                .unwrap();
            let asset: Value = serde_json::from_slice(&reply.body).unwrap();
            let rgba = asset["rgba"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            images.insert(
                &src,
                RgbaImage {
                    width: asset["width"].as_u64().unwrap() as u32,
                    height: asset["height"].as_u64().unwrap() as u32,
                    rgba,
                },
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
    let scroll = ScrollState::new();
    let mut cache = LayoutCache::default();
    let tree = cw_web::layout::layout_with(
        &doc,
        &styles,
        viewport,
        LayoutOptions {
            images: &images,
            scroll: &scroll,
        },
        &mut cache,
    );
    let scene = cw_web::paint::paint(&doc, &styles, &tree, viewport, &PaintContext::new(&images));
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
        .join("../../../research/studies/site-stills")
        .join(file);
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}
fn site(name: &str) -> (String, Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../worlds/company-2026/sites/{name}.json"));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let host = file["domains"][0].as_str().unwrap().to_owned();
    let state = SpeakerService
        .initialize(file["initial_state"].clone(), &ctx(0))
        .unwrap();
    (host, state)
}
/// The session a music player hands over: the album the seeded catalogues carry, so the
/// art the speaker draws is the art spotify.com draws for the same record.
fn session() -> Value {
    json!({
        "source": "http://spotify.com/",
        "player": {"item": "cold-reads", "queue": ["cold-reads", "warm-cache", "eviction", "tiling"],
                   "index": 1, "position_ms": 64_000, "playing": true, "volume": 70},
        "tracks": {
            "cold-reads": {"title": "Cold Reads", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 214_000},
            "warm-cache": {"title": "Warm Cache", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 196_000},
            "eviction": {"title": "Eviction", "artist": "Cache Miss", "album": "Cold Reads", "duration_ms": 243_000},
            "tiling": {"title": "Tiling", "artist": "Tessellate", "album": "Tiling", "duration_ms": 294_000}
        }
    })
}

/// `REPRO=/tmp/x.html` renders that file to `/tmp/x.html.png`: a bench for engine
/// reproductions.
#[test]
#[ignore]
fn repro() {
    let Ok(path) = std::env::var("REPRO") else {
        return;
    };
    let html = std::fs::read_to_string(&path).unwrap();
    let (host, mut state) = site("livingroom-speaker");
    let viewport = Viewport {
        width: 800,
        height: 400,
        scale: 1,
        zoom: 100,
    };
    render(&mut state, &host, &html, &format!("{path}.png"), viewport);
}

#[test]
#[ignore]
fn speaker_stills() {
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (name, playing) in [
        ("livingroom-speaker", true),
        ("kitchen-speaker", false),
        ("office-tv-speaker", true),
    ] {
        let (host, mut state) = site(name);
        if playing {
            let cast =
                HttpRequest::json("POST", format!("http://{host}/api/cast"), &session()).unwrap();
            SpeakerService.handle(&mut state, &ctx(0), &cast).unwrap();
        }
        let response = SpeakerService
            .handle(
                &mut state,
                &ctx(0),
                &HttpRequest::get(format!("http://{host}/")),
            )
            .unwrap();
        assert_eq!(response.status, 200, "{name}");
        render(
            &mut state,
            &host,
            &String::from_utf8(response.body).unwrap(),
            &format!("{name}.png"),
            viewport,
        );
    }
}
