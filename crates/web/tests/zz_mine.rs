//! Scratch probe for the second gap sweep.
#[cfg(feature = "pipeline")]
#[test]
fn probe() {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::dom::Document;
    use cw_web::{Strictness, Viewport};
    let vp = Viewport { width: 320, height: 200, scale: 1, zoom: 100 };
    let cases: [(&str, &str); 3] = [
        ("G text-shadow over", "<style>body{margin:0;background:#fff;font:48px Arial}p{margin:10px;text-shadow:0 0 0 #d00}</style><p>Ag</p>"),
        ("G outer shadow over", "<style>body{margin:0;background:#fff}div{margin:20px;width:100px;height:60px;background:#ddd;box-shadow:20px 20px 0 #0a0}</style><div></div>"),
        ("H big blur", "<style>body{margin:0;background:#888}div{margin:20px;width:100px;height:60px;background:#fff;box-shadow:0 26px 50px rgba(0,0,0,.55)}</style><div></div>"),
    ];
    for (label, body) in cases {
        let html = format!("<!doctype html><html><head>{body}");
        let doc = cw_web::html::parse(&html);
        let mut sheets = Vec::new();
        for n in doc.descendants(Document::ROOT) {
            if doc.is(n, "style") {
                sheets.push(parse_stylesheet(&doc.text_content(n), Origin::Author, Strictness::Lenient).unwrap());
            }
        }
        let media = Media::with_size(vp.width as i32, vp.height as i32);
        let styles = cw_web::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Lenient).unwrap();
        let tree = cw_web::layout::layout(&doc, &styles, vp);
        let scene = cw_web::paint::paint(&doc, &styles, &tree, vp, &cw_web::paint::PaintContext::default());
        let f = cw_render::Renderer::new().render(&scene);
        eprintln!("---- {label}");
        for n in &scene.nodes {
            let d = match &n.primitive {
                cw_scene::Primitive::Box { fill, .. } => format!("Box {fill:?}"),
                cw_scene::Primitive::RoundedBox { fill, border, border_width, radius } => format!("RoundedBox {fill:?} {border:?}/{border_width} r{radius}"),
                cw_scene::Primitive::Shadow { color, radius, blur } => format!("Shadow {color:?} r{radius} b{blur}"),
                cw_scene::Primitive::Path { points, fill, stroke, .. } => format!("Path n{} f{fill:?} s{stroke:?} {:?}", points.len(), &points[..points.len().min(4)]),
                cw_scene::Primitive::Text { text, color, .. } | cw_scene::Primitive::UiText { text, color, .. } | cw_scene::Primitive::UiTextBold { text, color, .. } => format!("Text {text:?} {color:?}"),
                o => format!("{:?}", std::mem::discriminant(o)),
            };
            eprintln!("  z{} {:?} {d}", n.z, n.bounds);
        }
        {
            let mut darkest = (255u32, 0usize, 0usize);
            for y in 0..vp.height as usize {
                for x in 0..vp.width as usize {
                    let i = (y * f.width as usize + x) * 4;
                    let l = f.rgba[i] as u32 + f.rgba[i + 1] as u32 + f.rgba[i + 2] as u32;
                    if l / 3 < darkest.0 { darkest = (l / 3, x, y); }
                }
            }
            eprintln!("  darkest {darkest:?}");
        }
        for y in 0..60usize {
            let mut row = String::new();
            for x in 0..140usize {
                let i = (y * f.width as usize + x) * 4;
                let (r, g, b) = (f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]);
                row.push(if (r, g, b) == (255, 255, 255) { '.' } else if r > 200 && g < 120 { 'R' } else if g > 130 && r < 120 { 'G' } else if b > 150 && r < 120 { 'B' } else if r > 200 && g > 100 && b < 100 { 'O' } else if r == g && g == b { '#' } else { '?' });
            }
            if row.contains(|c| c != '.') { eprintln!("  |{row}"); }
        }
    }
}
