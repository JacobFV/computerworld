//! Renders each webmail's main page through the engine to `research/site-stills/<site>.png`.
//! Ignored by default: pictures for the record, not a gate.
//! `cargo test -p cw-service-mail --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_mail::MailService;
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
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../research/site-stills")
        .join(file);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}
fn site(name: &str) -> Value {
    let path = format!(
        "{}/../../../worlds/company-2026/sites/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
#[ignore]
fn webmail_stills() {
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (name, actor, pages) in [
        (
            "google-mail",
            "alice",
            vec![
                ("/", ""),
                ("/?folder=inbox&thread=mail-1", "-thread"),
                ("/?folder=inbox&compose=1", "-compose"),
            ],
        ),
        (
            "outlook-mail",
            "bob",
            vec![
                ("/?folder=inbox&thread=mail-1", ""),
                ("/?folder=inbox&compose=1", "-compose"),
            ],
        ),
        (
            "mail-com",
            "carol",
            vec![("/?folder=inbox&thread=mail-2", ""), ("/", "-list")],
        ),
    ] {
        let ctx = ServiceContext {
            actor: actor.into(),
            source: "pc".into(),
            tick: 1,
            seed: 1,
            instance: name.into(),
        };
        let mut state = MailService
            .initialize(site(name)["initial_state"].clone(), &ctx)
            .unwrap();
        for (path, suffix) in pages {
            let response = MailService
                .handle(
                    &mut state,
                    &ctx,
                    &HttpRequest::get(format!("http://mail{path}")),
                )
                .unwrap();
            assert_eq!(response.status, 200, "{name} {path}");
            let html = String::from_utf8(response.body).unwrap();
            if let Ok(dir) = std::env::var("DUMP_HTML") {
                std::fs::write(format!("{dir}/{name}{suffix}.html"), &html).unwrap();
            }
            render(&html, &format!("{name}{suffix}.png"), viewport);
        }
    }
}

/// Renders `$SCRATCH_HTML` to `$SCRATCH_PNG`: a bench for isolating an engine behaviour.
#[test]
#[ignore]
fn scratch() {
    let (Ok(html), Ok(png)) = (std::env::var("SCRATCH_HTML"), std::env::var("SCRATCH_PNG")) else {
        return;
    };
    let width = std::env::var("SCRATCH_W")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(640);
    let viewport = Viewport {
        width,
        height: width * 5 / 8,
        scale: 1,
        zoom: 100,
    };
    render(&std::fs::read_to_string(html).unwrap(), &png, viewport);
}
