//! Renders each bank site's main page through the engine to `research/studies/site-stills/`:
//! `northwind.png` and `paypal.png`, with the transfer pages beside them. Ignored by
//! default: pictures for the record, not a gate.
//! `cargo test -p cw-service-bank --test still -- --ignored`.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_bank::BankService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

const NORTHWIND: &str = include_str!("../../../../worlds/internet/sites/northwind.json");
const PAYPAL: &str = include_str!("../../../../worlds/internet/sites/paypal.json");

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
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../research/studies/site-stills");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join(file);
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn bank_site_stills() {
    let ctx = ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "bank".into(),
    };
    let viewport = Viewport {
        width: 1280,
        height: 800,
        scale: 1,
        zoom: 100,
    };
    for (site, host, name) in [
        (NORTHWIND, "northwind.example", "northwind"),
        (PAYPAL, "paypal.com", "paypal"),
    ] {
        let seed: Value = serde_json::from_str(site).unwrap();
        let mut state = BankService
            .initialize(seed["initial_state"].clone(), &ctx)
            .unwrap();
        let account = if name == "paypal" {
            "pp-alice"
        } else {
            "cc-3310"
        };
        for (path, file) in [
            ("/".to_owned(), format!("{name}.png")),
            ("/transfers".to_owned(), format!("{name}-transfers.png")),
            (
                format!("/accounts/{account}"),
                format!("{name}-account.png"),
            ),
            (
                format!("/accounts/{account}/transactions/tx-1"),
                format!("{name}-transaction.png"),
            ),
        ] {
            let response = BankService
                .handle(
                    &mut state,
                    &ctx,
                    &HttpRequest::get(format!("http://{host}{path}")),
                )
                .unwrap();
            assert_eq!(response.status, 200, "{host}{path}");
            render(&String::from_utf8(response.body).unwrap(), &file, viewport);
        }
    }
}
