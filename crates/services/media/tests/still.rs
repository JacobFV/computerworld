//! Renders each media site's pages through the engine to `research/studies/site-stills/`.
//! Ignored by default: pictures for the record, not a gate.
//! `cargo test -p cw-service-media --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_media::MediaService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::layout::{LayoutCache, LayoutOptions, ScrollState};
use cw_web::paint::{ImageMap, PaintContext, RgbaImage};
use cw_web::{Strictness, Viewport};
use serde_json::{json, Value};
use std::path::PathBuf;

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "media".into(),
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
            let reply = MediaService
                .handle(
                    state,
                    &ctx(),
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
fn site(name: &str) -> (Value, String, Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../worlds/internet/sites/{name}.json"));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let host = file["domains"][0].as_str().unwrap().to_owned();
    let state = MediaService
        .initialize(file["initial_state"].clone(), &ctx())
        .unwrap();
    (file.clone(), host, state)
}

/// `REPRO=/tmp/x.html` renders that file to `/tmp/x.html.png`: a bench for engine reproductions.
#[test]
#[ignore]
fn repro() {
    let Ok(path) = std::env::var("REPRO") else {
        return;
    };
    let html = std::fs::read_to_string(&path).unwrap();
    let (_, host, mut state) = site("youtube");
    let viewport = Viewport {
        width: 800,
        height: 400,
        scale: 1,
        zoom: 100,
    };
    render(&mut state, &host, &html, &format!("{path}.png"), viewport);
}

/// `STILLS=youtube,spotify` renders only those sites; `STILL_PAGES=main` only the main page.
#[test]
#[ignore]
fn site_stills() {
    let only = std::env::var("STILLS").unwrap_or_default();
    let main_only = std::env::var("STILL_PAGES").is_ok_and(|p| p == "main");
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for name in [
        "youtube",
        "netflix",
        "twitch",
        "vimeo",
        "tiktok",
        "spotify",
        "soundcloud",
        "youtube-music",
    ] {
        if !only.is_empty() && !only.split(',').any(|s| s == name) {
            continue;
        }
        let (_, host, mut state) = site(name);
        let mode = state["mode"].as_str().unwrap().to_owned();
        let first_item = state["items"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        let first_channel = state["channels"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        let mut pages = vec![("/".to_owned(), format!("{name}.png"))];
        if mode != "video" {
            // Something playing, so the pinned player bar is in the picture.
            let first_album = state["items"][&first_item]["album"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let _ = first_album;
            let request = HttpRequest::json(
                "POST",
                format!("http://{host}/api/player"),
                &json!({"action": "play", "item": first_item}),
            )
            .unwrap();
            MediaService.handle(&mut state, &ctx(), &request).unwrap();
        }
        if !main_only {
            match mode.as_str() {
                "video" => {
                    pages.push((
                        format!("/watch?v={first_item}"),
                        format!("{name}-watch.png"),
                    ));
                    pages.push((
                        format!("/channel/{first_channel}"),
                        format!("{name}-channel.png"),
                    ));
                    pages.push((
                        "/results?search_query=a".to_owned(),
                        format!("{name}-results.png"),
                    ));
                }
                "audio" => {
                    let playlist = state["playlists"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .next_back()
                        .unwrap()
                        .clone();
                    pages.push((
                        format!("/playlist/{playlist}"),
                        format!("{name}-playlist.png"),
                    ));
                    pages.push((
                        format!("/artist/{first_channel}"),
                        format!("{name}-artist.png"),
                    ));
                }
                _ => {
                    pages.push((
                        format!("/watch?v={first_item}"),
                        format!("{name}-watch.png"),
                    ));
                    pages.push((
                        format!("/channel/{first_channel}"),
                        format!("{name}-artist.png"),
                    ));
                }
            }
        }
        for (path, file) in pages {
            let mut request = HttpRequest::get(format!("http://{host}{path}"));
            // A refresh, so rendering a watch page for the picture plays nothing.
            request
                .headers
                .insert(cw_protocol::REFRESH_HEADER.into(), "1".into());
            let response = MediaService.handle(&mut state, &ctx(), &request).unwrap();
            assert_eq!(response.status, 200, "{host}{path}");
            render(
                &mut state,
                &host,
                &String::from_utf8(response.body).unwrap(),
                &file,
                viewport,
            );
        }
    }
}
