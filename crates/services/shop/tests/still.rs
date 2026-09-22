//! Renders each storefront through the engine to `research/studies/site-stills/<site>.png` (the
//! home page) plus a few inner pages. Ignored by default: pictures for the record, not
//! a gate. `cargo test -p cw-service-shop --test still -- --ignored`; `STILL_SITES=amazon,etsy`
//! narrows the run.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_shop::ShopService;
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
        .join("../../../research/studies/site-stills")
        .join(file);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

/// An inner page: file suffix, path, and whose session it is.
type Inner = (&'static str, &'static str, &'static str);
/// (site, host, inner pages).
const SITES: &[(&str, &str, &[Inner])] = &[
    (
        "amazon",
        "amazon.com",
        &[
            ("detail", "/dp/b0monitor27", "alice"),
            ("cart", "/cart", "carol"),
            ("orders", "/orders", "alice"),
        ],
    ),
    (
        "ebay",
        "ebay.com",
        &[("detail", "/dp/115402998811", "alice")],
    ),
    (
        "etsy",
        "etsy.com",
        &[("detail", "/dp/et-mug-slab", "alice")],
    ),
    (
        "airbnb",
        "airbnb.com",
        &[("detail", "/dp/cabin-skykomish-aframe", "bob")],
    ),
    (
        "booking",
        "booking.com",
        &[("detail", "/dp/lis-miradouro-hotel", "carol")],
    ),
    ("uber", "uber.com", &[("cart", "/cart", "bob")]),
    ("doordash", "doordash.com", &[("cart", "/cart", "bob")]),
    (
        "ticketmaster",
        "ticketmaster.com",
        &[
            ("event", "/event/devcon-2026", "carol"),
            ("order", "/orders/TM-2210", "carol"),
        ],
    ),
];

#[test]
#[ignore]
fn storefront_stills() {
    let only: Vec<String> = std::env::var("STILL_SITES")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let viewport = Viewport {
        width: 1280,
        height: 900,
        scale: 1,
        zoom: 100,
    };
    for (site, host, inner) in SITES {
        if !only.is_empty() && !only.iter().any(|s| s == site) {
            continue;
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../../worlds/company-2026/sites/{site}.json"));
        let seed: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ctx = |actor: &str| ServiceContext {
            actor: actor.into(),
            source: "alice-mac".into(),
            tick: 1,
            seed: 1,
            instance: (*site).into(),
        };
        let mut state = ShopService
            .initialize(seed["initial_state"].clone(), &ctx("alice"))
            .unwrap();
        let mut shoot = |url: String, actor: &str, file: String| {
            let response = ShopService
                .handle(&mut state, &ctx(actor), &HttpRequest::get(&url))
                .unwrap();
            assert_eq!(response.status, 200, "{url}");
            render(&String::from_utf8(response.body).unwrap(), &file, viewport);
        };
        shoot(format!("http://{host}/"), "alice", format!("{site}.png"));
        if std::env::var("STILL_HOME_ONLY").is_err() {
            for (suffix, page, actor) in *inner {
                shoot(
                    format!("http://{host}{page}"),
                    actor,
                    format!("{site}-{suffix}.png"),
                );
            }
        }
    }
}

/// `STILL_HTML=/path/in.html STILL_OUT=name.png`: renders one file, for engine-gap reproductions.
#[test]
#[ignore]
fn scratch() {
    let Ok(input) = std::env::var("STILL_HTML") else {
        return;
    };
    let out = std::env::var("STILL_OUT").unwrap_or_else(|_| "scratch.png".into());
    render(
        &std::fs::read_to_string(input).unwrap(),
        &out,
        Viewport {
            width: 600,
            height: 300,
            scale: 1,
            zoom: 100,
        },
    );
}
