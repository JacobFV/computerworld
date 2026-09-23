//! Renders every publication's front page (and one article each) through the engine to
//! `research/studies/site-stills/<site>.png`. Ignored by default: pictures for the record, not
//! a gate. `cargo test -p cw-service-press --test still -- --ignored`; `PRESS_STILL=bbc,cnn`
//! renders only those sites.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_press::PressService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::{Strictness, Viewport};
use serde_json::Value;
use std::path::PathBuf;

static SITES: std::sync::LazyLock<Vec<(&'static str, &'static str)>> =
    std::sync::LazyLock::new(|| {
        vec![
            (
                "nytimes",
                cw_service_common::reference::reference_site_json("nytimes"),
            ),
            (
                "bbc",
                cw_service_common::reference::reference_site_json("bbc"),
            ),
            (
                "cnn",
                cw_service_common::reference::reference_site_json("cnn"),
            ),
            (
                "reuters",
                cw_service_common::reference::reference_site_json("reuters"),
            ),
            (
                "theverge",
                cw_service_common::reference::reference_site_json("theverge"),
            ),
            (
                "arstechnica",
                cw_service_common::reference::reference_site_json("arstechnica"),
            ),
            (
                "google-news",
                cw_service_common::reference::reference_site_json("google-news"),
            ),
            (
                "medium",
                cw_service_common::reference::reference_site_json("medium"),
            ),
            (
                "substack",
                cw_service_common::reference::reference_site_json("substack"),
            ),
            (
                "alice-blog",
                include_str!("../../../../worlds/company-2026/sites/alice-blog.json"),
            ),
            (
                "bob-blog",
                include_str!("../../../../worlds/company-2026/sites/bob-blog.json"),
            ),
            (
                "northstar-eng",
                include_str!("../../../../worlds/company-2026/sites/northstar-eng.json"),
            ),
        ]
    });

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
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}

#[test]
#[ignore]
fn press_site_stills() {
    // `PRESS_STILL_HTML=/path/to/page.html` renders that file instead (to `scratch.png`): the
    // way to picture a minimal reproduction of something the engine draws wrong.
    if let Ok(path) = std::env::var("PRESS_STILL_HTML") {
        let html = std::fs::read_to_string(&path).unwrap();
        render(
            &html,
            "scratch.png",
            Viewport {
                width: 400,
                height: 200,
                scale: 1,
                zoom: 100,
            },
        );
        return;
    }
    let only = std::env::var("PRESS_STILL").ok();
    let ctx = ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "press".into(),
    };
    // `PRESS_STILL_HEIGHT` renders a taller frame, to look at the foot of a long page.
    let height = std::env::var("PRESS_STILL_HEIGHT")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or(1100);
    let viewport = Viewport {
        width: 1280,
        height,
        scale: 1,
        zoom: 100,
    };
    for (site, source) in SITES.iter().copied() {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|s| s == site))
        {
            continue;
        }
        let file: Value = serde_json::from_str(source).unwrap();
        let origin = format!("http://{}", file["domains"][0].as_str().unwrap());
        let mut state = PressService
            .initialize(file["initial_state"].clone(), &ctx)
            .unwrap();
        let front = PressService
            .handle(&mut state, &ctx, &HttpRequest::get(format!("{origin}/")))
            .unwrap();
        assert_eq!(front.status, 200);
        let front = String::from_utf8(front.body).unwrap();
        render(&front, &format!("{site}.png"), viewport);
        // The newest story, by the href of the first card on the front page.
        let at = front.find("class=\"card").unwrap();
        let href = front[at..]
            .split("href=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let story = PressService
            .handle(
                &mut state,
                &ctx,
                &HttpRequest::get(format!("{origin}{href}")),
            )
            .unwrap();
        assert_eq!(story.status, 200);
        render(
            &String::from_utf8(story.body).unwrap(),
            &format!("{site}-article.png"),
            viewport,
        );
    }
}
