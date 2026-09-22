//! Renders the two drives' main pages through the engine to `research/site-stills/`. Ignored by
//! default: pictures for the record, not a gate.
//! `cargo test -p cw-service-drive --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_drive::DriveService;
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
        .join("../../research/site-stills")
        .join(file);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn drive_and_dropbox_stills() {
    let world: Value = serde_json::from_str(WORLD).unwrap();
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (site, actor, pages) in [
        (
            "google-drive",
            "alice",
            &[
                ("http://drive.google.com/", "google-drive.png"),
                (
                    "http://drive.google.com/drive/folders/f-atlas",
                    "google-drive-folder.png",
                ),
                (
                    "http://drive.google.com/file/press-kit",
                    "google-drive-file.png",
                ),
            ][..],
        ),
        (
            "dropbox",
            "carol",
            &[
                ("http://dropbox.com/", "dropbox.png"),
                (
                    "http://dropbox.com/drive/folders/atlas-assets",
                    "dropbox-folder.png",
                ),
                ("http://dropbox.com/file/press-kit", "dropbox-file.png"),
            ][..],
        ),
    ] {
        let service = world["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == site)
            .unwrap();
        let ctx = ServiceContext {
            actor: actor.into(),
            source: "alice-mac".into(),
            tick: 1,
            seed: 1,
            instance: site.into(),
        };
        let mut state = DriveService
            .initialize(service["initial_state"].clone(), &ctx)
            .unwrap();
        for (url, file) in pages {
            let response = DriveService
                .handle(&mut state, &ctx, &HttpRequest::get(*url))
                .unwrap();
            assert_eq!(response.status, 200, "{url}");
            render(&String::from_utf8(response.body).unwrap(), file, viewport);
        }
    }
}
