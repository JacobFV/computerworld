//! A phone is a supported viewport in this world, so every page of every skin has to be
//! readable on one. Each page is laid out at the two widths a phone actually has and the
//! laid-out content may not be wider than the screen: a page that overflows sideways is
//! clipped on both edges, and the text in the middle column loses its first words on every
//! line.
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_media::MediaService;
use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
use cw_web::dom::Document;
use cw_web::layout::fragment::{Fragment, FragmentKind};
use cw_web::{Strictness, Viewport};
use serde_json::Value;

const SITES: [&str; 8] = [
    "youtube",
    "netflix",
    "twitch",
    "vimeo",
    "tiktok",
    "spotify",
    "soundcloud",
    "youtube-music",
];
/// The two phone widths this world's device set uses, and a small tablet.
const WIDTHS: [u32; 3] = [390, 412, 768];

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 12,
        seed: 1,
        instance: "media".into(),
    }
}
/// How far right the page's boxes reach when it is laid out `width` pixels wide, and which
/// box reaches furthest. Boxes, not text runs: a preserved trailing space hangs past the
/// end of its line by design and is not what clips a page.
fn widest_box(html: &str, width: u32) -> (f64, String) {
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
    let viewport = Viewport {
        width,
        height: 844,
        scale: 1,
        zoom: 100,
    };
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
    fn visit(doc: &Document, f: &Fragment, ox: f64, worst: &mut (f64, String)) {
        let x = ox + f.rect.origin.x.to_f64_px();
        // What sits inside a scroll box of the page's own — a `<pre>` of code, a rail that
        // scrolls — is reachable by scrolling it, so it is not what clips the page. The
        // document's own scroll box is not one of those: a page wider than the screen is
        // exactly the fault this test is for.
        if let FragmentKind::Box {
            scroll: Some(_),
            source,
            ..
        } = &f.kind
        {
            let tag = doc.tag(source.node()).unwrap_or("");
            // The document's own box has no tag; `html` and `body` scroll the page itself.
            if !matches!(tag, "" | "html" | "body") {
                return;
            }
        }
        if let FragmentKind::Box { source, .. } | FragmentKind::InlineBox { source, .. } = &f.kind {
            let right = x + f.rect.size.width.to_f64_px();
            if right > worst.0 {
                let node = source.node();
                *worst = (
                    right,
                    format!(
                        "<{}{}{}>",
                        doc.tag(node).unwrap_or("?"),
                        doc.attr(node, "id")
                            .map(|i| format!(" id={i:?}"))
                            .unwrap_or_default(),
                        doc.attr(node, "class")
                            .map(|c| format!(" class={c:?}"))
                            .unwrap_or_default()
                    ),
                );
            }
        }
        for child in &f.children {
            visit(doc, child, x, worst);
        }
    }
    let mut worst = (0.0, String::new());
    visit(&doc, &tree.root, 0.0, &mut worst);
    worst
}

#[test]
fn every_page_of_every_skin_fits_a_phone() {
    for site in SITES {
        let path = cw_service_common::reference::reference_site_path(site);
        let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let domain = file["domains"][0].as_str().unwrap().to_owned();
        let mode = file["initial_state"]["mode"]
            .as_str()
            .unwrap_or("video")
            .to_owned();
        let mut state = MediaService
            .initialize(file["initial_state"].clone(), &ctx())
            .unwrap();
        let seed = state.clone();
        let mut paths: Vec<String> = match mode.as_str() {
            "audio" => vec![
                "/".into(),
                "/search".into(),
                "/search?q=a".into(),
                "/collection".into(),
                "/queue".into(),
                "/lyrics".into(),
            ],
            "music" => vec![
                "/".into(),
                "/explore".into(),
                "/library".into(),
                "/search?q=a".into(),
            ],
            _ => vec![
                "/".into(),
                "/results?search_query=a".into(),
                "/playlists".into(),
                "/playlist?list=watch-later".into(),
            ],
        };
        for (id, _) in seed["items"].as_object().into_iter().flatten().take(4) {
            paths.push(match mode.as_str() {
                "audio" => format!("/track/{id}"),
                "music" => format!("/watch?v={id}"),
                _ => format!("/watch?v={id}"),
            });
        }
        for (id, _) in seed["channels"].as_object().into_iter().flatten().take(3) {
            paths.push(match mode.as_str() {
                "audio" => format!("/artist/{id}"),
                _ => format!("/channel/{id}"),
            });
        }
        for path in &paths {
            let url = format!("http://{domain}{path}");
            let reply = MediaService
                .handle(&mut state, &ctx(), &HttpRequest::get(&url))
                .unwrap();
            assert_eq!(reply.status, 200, "{url}");
            let body = String::from_utf8(reply.body).unwrap();
            for width in WIDTHS {
                let (right, which) = widest_box(&body, width);
                assert!(
                    right <= f64::from(width) + 0.5,
                    "{site} {path} at {width}px wide: {which} reaches {right}px, past the edge of \
                     the screen — the page is clipped on a phone"
                );
            }
        }
    }
}
