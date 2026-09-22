//! Renders each geo site's main page through the engine to `research/studies/site-stills/<site>.png`.
//! The map a page names is fetched from the same service (`GET /map.rgba`) and handed to
//! layout and paint the way the browser does. Ignored by default: pictures for the record,
//! not a gate. `cargo test -p cw-service-geo --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_geo::GeoService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::layout::{LayoutCache, LayoutOptions, ScrollState};
use cw_web::paint::{ImageMap, PaintContext, RgbaImage};
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "geo".into(),
    }
}
fn fetch(state: &mut Value, actor: &str, url: &str) -> Vec<u8> {
    let r = GeoService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap();
    assert_eq!(r.status, 200, "{url}");
    r.body
}
fn render(state: &mut Value, actor: &str, origin: &str, path: &str, file: &str) {
    let html = String::from_utf8(fetch(state, actor, &format!("{origin}{path}"))).unwrap();
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../research/studies/site-stills")
        .join(file);
    draw(&html, &target, |src| {
        fetch(state, actor, &format!("{origin}{src}"))
    });
}
/// HTML to a PNG at 1280x800; `image` answers each `<img src>` with an RGBA asset.
fn draw(html: &str, target: &std::path::Path, mut image: impl FnMut(&str) -> Vec<u8>) {
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
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
        if let Some(src) = doc.attr(node, "src").filter(|_| doc.is(node, "img")) {
            let asset: Value = serde_json::from_slice(&image(src)).unwrap();
            let rgba = asset["rgba"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect();
            images.insert(
                src,
                RgbaImage::new(
                    asset["width"].as_u64().unwrap() as u32,
                    asset["height"].as_u64().unwrap() as u32,
                    rgba,
                ),
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
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&frame.rgba)
            .unwrap();
    }
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn geo_site_stills() {
    let sites = [
        (
            "google-maps",
            include_str!("../../../../worlds/company-2026/sites/google-maps.json"),
            "http://maps.google.com",
            "carol",
        ),
        (
            "osm",
            include_str!("../../../../worlds/company-2026/sites/osm.json"),
            "http://openstreetmap.org",
            "bob",
        ),
        (
            "weather",
            include_str!("../../../../worlds/company-2026/sites/weather.json"),
            "http://weather.com",
            "alice",
        ),
    ];
    let only = std::env::var("STILL").ok();
    for (site, raw, origin, actor) in sites {
        let seed: Value = serde_json::from_str(raw).unwrap();
        let mut state = GeoService
            .initialize(seed["initial_state"].clone(), &ctx(actor))
            .unwrap();
        let mut pages = vec![("/".to_owned(), format!("{site}.png"))];
        if seed["initial_state"]["mode"] == "maps" {
            pages.push((
                "/maps/place/harborline-coffee".into(),
                format!("{site}-place.png"),
            ));
            pages.push((
                "/maps/dir?from=northstar-hq&to=devcon-center&mode=driving".into(),
                format!("{site}-directions.png"),
            ));
        } else {
            pages.push((
                "/weather/tenday/l/seattle".into(),
                format!("{site}-tenday.png"),
            ));
        }
        for (path, file) in pages {
            if only.as_deref().is_none_or(|o| file.starts_with(o)) {
                render(&mut state, actor, origin, &path, &file);
            }
        }
    }
}

/// An engine-gap reproduction: `STILL_HTML=<file> STILL_OUT=<png>` renders any page.
#[test]
#[ignore]
fn render_a_file() {
    let (Ok(source), Ok(out)) = (std::env::var("STILL_HTML"), std::env::var("STILL_OUT")) else {
        return;
    };
    draw(
        &std::fs::read_to_string(source).unwrap(),
        std::path::Path::new(&out),
        |_| unreachable!("no images"),
    );
}
