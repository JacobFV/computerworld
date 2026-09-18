use super::*;
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::{AppEnv, SystemSettings};

const PRODUCTS: [(Product, DesktopTheme); 8] = [
    (Product::Paint, DesktopTheme::Windows),
    (Product::Preview, DesktopTheme::Macos),
    (Product::Pixelmator, DesktopTheme::Macos),
    (Product::Gimp, DesktopTheme::Ubuntu),
    (Product::Pinta, DesktopTheme::Ubuntu),
    (Product::Sketchbook, DesktopTheme::Android),
    (Product::IosPhotos, DesktopTheme::Ios),
    (Product::GooglePhotos, DesktopTheme::Android),
];

fn env(theme: DesktopTheme, width: u32, height: u32) -> AppEnv<'static> {
    AppEnv {
        theme,
        width,
        height,
        clock_us: 0,
        settings: &SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}

/// A studio with a small white image open.
fn studio(product: Product) -> Studio {
    let mut s = Studio::new(product);
    s.doc = Some(Document::new(40, 30, Some(cw_raster::WHITE)).unwrap());
    s.tool = product.tools()[0];
    if matches!(product, Product::IosPhotos | Product::GooglePhotos) {
        s.path = "Pictures/photo.png".into();
        s.tab = "adjust".into();
    }
    s
}

fn targets(s: &Studio, theme: DesktopTheme) -> Vec<String> {
    let mut p = Painter::themed(theme, 1100, 760, 0);
    s.render(&mut p, &env(theme, 1100, 760));
    p.scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}

fn command(s: &mut Studio, target: &str) -> Result<Vec<AppEffect>, String> {
    let c = target
        .strip_prefix(s.product.prefix())
        .and_then(|t| t.strip_prefix(':'))
        .ok_or_else(|| format!("{target} is not this product's"))?
        .to_owned();
    s.command(1, &c)
}

/// Every state worth drawing: plain, each menu open, each dialog open, the file
/// sheets, and each phone tab.
fn states(product: Product) -> Vec<Studio> {
    let base = studio(product);
    let mut out = vec![base.clone()];
    // Products that start empty have a state with nothing open; the rest never do.
    if product.blank().is_none() && !product.mobile() {
        out.push(Studio::new(product));
    }
    for id in [
        "file",
        "edit",
        "view",
        "select",
        "image",
        "layer",
        "colors",
        "tools",
        "filters",
        "selection",
        "rotate",
        "brushes",
        "outline",
        "fill",
        "shapes",
        "size",
        "blend",
        "adjustments",
        "effects",
        "main",
        "zoom",
        "style",
        "text-style",
        "border",
        "fill-color",
        "brush",
        "colors",
    ] {
        let mut s = base.clone();
        s.panel = Some(Panel::Menu { id: id.into() });
        out.push(s);
    }
    for id in product.dialogs() {
        let mut s = base.clone();
        s.open_dialog(id).unwrap();
        out.push(s);
    }
    let mut open = base.clone();
    open.panel = Some(Panel::Open {
        folder: "/home/alice/Pictures".into(),
        entries: vec!["a.png".into(), "trips/".into()],
        loading: false,
    });
    out.push(open);
    let mut save = base.clone();
    save.panel = Some(Panel::Save {
        folder: "Pictures".into(),
        name: "x.png".into(),
        entries: vec!["x.png".into()],
        export: false,
    });
    out.push(save);
    for tab in [
        "adjust",
        "filters",
        "crop",
        "markup",
        "suggestions",
        "undo",
        "effects",
    ] {
        let mut s = base.clone();
        s.tab = tab.into();
        out.push(s);
    }
    for tool in product.tools() {
        let mut s = base.clone();
        s.tool = *tool;
        out.push(s);
    }
    // A save sheet for each format the product writes, and GIMP's Export sheet.
    for export in [false, true] {
        for format in product.save_formats(export) {
            let mut s = base.clone();
            s.panel = Some(Panel::Save {
                folder: "Pictures".into(),
                name: format!("x.{}", format.extension()),
                entries: vec![],
                export,
            });
            out.push(s);
        }
    }
    // Export options waiting on a JPEG, a path being drawn, a curve being bent, a
    // clone source set, and the Paths tab.
    let mut jpeg = base.clone();
    jpeg.pending = Some("Pictures/x.jpg".into());
    jpeg.show_dialog(
        if product == Product::Gimp {
            "jpeg"
        } else {
            "jpeg-quality"
        },
        BTreeMap::new(),
    )
    .unwrap();
    out.push(jpeg);
    for tool in [
        Tool::Paths,
        Tool::Clone,
        Tool::Heal,
        Tool::Gradient,
        Tool::Shape,
    ] {
        if !product.tools().contains(&tool) {
            continue;
        }
        let mut s = base.clone();
        s.tool = tool;
        s.tab = "paths".into();
        s.retouch.source = Some((3, 4));
        s.path_edit = Some(Path {
            anchors: vec![
                Anchor::corner(centre(2, 2)),
                Anchor::corner(centre(20, 2)),
                Anchor::corner(centre(20, 20)),
            ],
            closed: false,
        });
        s.curve = Some(CurveEdit {
            points: vec![centre(1, 1), centre(5, 5), centre(9, 1), centre(12, 9)],
            bends: 1,
        });
        out.push(s);
    }
    let mut layered = base;
    layered.layers_open = true;
    layered.doc.as_mut().unwrap().add_layer("Layer 1").unwrap();
    out.push(layered);
    out
}

#[test]
fn every_painted_control_is_one_the_product_handles() {
    for (product, theme) in PRODUCTS {
        for state in states(product) {
            for target in targets(&state, theme) {
                let mut s = state.clone();
                // Look commands that save belong to Photos, which wraps the phone editors.
                if target.ends_with(":look:done") || target.ends_with(":discard") {
                    continue;
                }
                assert!(
                    command(&mut s, &target).is_ok(),
                    "{product:?}: painted control {target} is refused: {:?}",
                    command(&mut s.clone(), &target)
                );
            }
        }
    }
}

#[test]
fn products_refuse_what_they_do_not_have() {
    let mut paint = studio(Product::Paint);
    assert!(paint.command(1, "dialog:gaussian-blur").is_err());
    assert!(paint.command(1, "action:invert").is_err());
    assert!(paint.command(1, "tool:wand").is_err());
    let mut gimp = studio(Product::Gimp);
    assert!(gimp.command(1, "dialog:gaussian-blur").is_ok());
    assert!(
        gimp.command(1, "shape:star").is_err(),
        "GIMP has no shape tools"
    );
    let mut preview = studio(Product::Preview);
    assert!(
        preview.command(1, "layer:new").is_err(),
        "Preview has no layers"
    );
    assert!(preview.command(1, "dialog:adjust-color").is_ok());
}

#[test]
fn a_pointer_drag_paints_through_the_view_mapping() {
    let mut s = studio(Product::Paint);
    s.tool = Tool::Pencil;
    s.size = 1;
    s.zoom = 200;
    // A 100x100 view of a 40x30 image at 200%: the image is 80x60, centred at (10, 20).
    assert_eq!(s.origin(100, 100), (10, 20));
    assert_eq!(s.to_image(100, 100, 11, 21), (8, 8));
    s.pointer(1, "canvas:100:100", PointerPhase::Down, 11, 21)
        .unwrap();
    s.pointer(1, "canvas:100:100", PointerPhase::Move, 31, 21)
        .unwrap();
    s.pointer(1, "canvas:100:100", PointerPhase::Up, 51, 21)
        .unwrap();
    let doc = s.doc.as_ref().unwrap();
    for x in 0..=20 {
        assert_eq!(doc.pixel(x, 0), cw_raster::BLACK, "pixel {x}");
    }
    assert_eq!(doc.pixel(21, 0), cw_raster::WHITE);
    assert!(s.modified);
    assert_eq!(doc.undo_label(), Some("Pencil"));
    s.command(1, "undo").unwrap();
    assert_eq!(s.doc.as_ref().unwrap().pixel(5, 0), cw_raster::WHITE);
    s.command(1, "redo").unwrap();
    assert_eq!(s.doc.as_ref().unwrap().pixel(5, 0), cw_raster::BLACK);
}

#[test]
fn shapes_selections_and_fills_come_from_drags_and_clicks() {
    let mut s = studio(Product::Pinta);
    s.command(1, "shape:rectangle").unwrap();
    s.command(1, "fill-style:both").unwrap();
    s.command(1, "color:ff0000").unwrap();
    s.secondary = [0, 0, 255, 255];
    s.size = 1;
    s.antialias = false;
    s.zoom = 100;
    // 40x30 at 100% in a 40x30 view: view and image pixels coincide.
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 5, 5)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Up, 14, 14)
        .unwrap();
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(5, 9), [255, 0, 0, 255], "outline");
    assert_eq!(doc.pixel(9, 9), [0, 0, 255, 255], "fill");
    assert_eq!(doc.pixel(20, 20), cw_raster::WHITE);
    s.command(1, "tool:select-rect").unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 20, 0)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Up, 29, 9)
        .unwrap();
    let sel = s.doc.as_ref().unwrap().selection().and_then(Mask::bounds);
    assert_eq!(sel, Some(IRect::new(20, 0, 10, 10)));
    s.command(1, "tool:fill").unwrap();
    s.command(1, "color:00ff00").unwrap();
    s.command(1, "canvas:40:30").unwrap();
    // The fill started at (0, 0), outside the selection, so it only lands inside it.
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(25, 5), [0, 255, 0, 255]);
    assert_eq!(doc.pixel(0, 0), cw_raster::WHITE);
    // A click with the rectangle tool drops the selection.
    s.command(1, "tool:select-rect").unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 3, 3)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Up, 3, 3)
        .unwrap();
    assert!(s.doc.as_ref().unwrap().selection().is_none());
}

#[test]
fn dialogs_preview_then_apply_exactly_once() {
    let mut s = studio(Product::Gimp);
    s.doc = Some(Document::new(4, 4, Some([100, 100, 100, 255])).unwrap());
    s.command(1, "dialog:brightness-contrast").unwrap();
    s.command(1, "set:brightness:50").unwrap();
    // Previewed, not yet applied.
    assert_eq!(
        s.preview_layer().map(|l| l.get(0, 0)),
        Some([178, 178, 178, 255])
    );
    assert_eq!(s.doc.as_ref().unwrap().pixel(0, 0), [100, 100, 100, 255]);
    s.command(1, "apply").unwrap();
    // 100 + (255 - 100) / 2 = 177.5 -> 178.
    assert_eq!(s.doc.as_ref().unwrap().pixel(0, 0), [178, 178, 178, 255]);
    assert!(s.panel.is_none());
    assert_eq!(
        s.doc.as_ref().unwrap().undo_label(),
        Some("Brightness-Contrast")
    );
    // Sliders set the value where they are pressed.
    s.command(1, "dialog:posterize").unwrap();
    s.pointer(1, "slider:levels:62", PointerPhase::Down, 0, 0)
        .unwrap();
    assert_eq!(s.param("levels"), 2);
    s.pointer(1, "slider:levels:62", PointerPhase::Up, 62, 0)
        .unwrap();
    assert_eq!(s.param("levels"), 64);
    s.command(1, "cancel").unwrap();
    // Resizing keeps the aspect ratio when asked.
    s.doc = Some(Document::new(40, 20, None).unwrap());
    s.command(1, "dialog:resize").unwrap();
    s.command(1, "set:width:80").unwrap();
    assert_eq!(s.param("height"), 40);
    s.command(1, "apply").unwrap();
    assert_eq!(s.doc.as_ref().unwrap().width(), 80);
    // A one-shot action applies at once.
    s.command(1, "action:invert").unwrap();
    assert_eq!(s.doc.as_ref().unwrap().undo_label(), Some("Invert"));
}

#[test]
fn saving_names_a_png_and_marks_the_document_clean() {
    let mut s = studio(Product::Paint);
    s.modified = true;
    let effects = s.command(1, "save").unwrap();
    // Untitled: a save sheet, which lists its folder first.
    assert!(matches!(&effects[0], AppEffect::ListDirectory { path, .. } if path == "Pictures"));
    s.listed(vec![
        "Untitled.png".into(),
        "notes.txt".into(),
        "trips/".into(),
    ]);
    let Some(Panel::Save { entries, .. }) = &s.panel else {
        panic!("no save sheet");
    };
    assert_eq!(
        entries,
        &vec!["Untitled.png".to_string(), "trips/".to_string()]
    );
    s.key(1, "Backspace").unwrap();
    for _ in 0..11 {
        s.key(1, "Backspace").unwrap();
    }
    s.text("drawing").unwrap();
    let effects = s.command(1, "save-confirm").unwrap();
    let AppEffect::WriteImage {
        path,
        width,
        height,
        rgba,
        ..
    } = &effects[0]
    else {
        panic!("not a write");
    };
    assert_eq!(path, "Pictures/drawing.png");
    assert_eq!((*width, *height), (40, 30));
    assert_eq!(rgba.len(), 40 * 30 * 4);
    s.saved(path);
    assert!(!s.modified);
    assert_eq!(s.path, "Pictures/drawing.png");
    assert!(s.panel.is_none());
    // Now it saves in place.
    let effects = s.command(1, "save").unwrap();
    assert!(
        matches!(&effects[0], AppEffect::WriteImage { path, .. } if path == "Pictures/drawing.png")
    );
    // A JPEG original saves in place as a JPEG, never as PNG bytes.
    s.path = "Pictures/photo.jpg".into();
    let effects = s.command(1, "save").unwrap();
    let AppEffect::WriteBytes { path, bytes, .. } = &effects[0] else {
        panic!("not a byte write");
    };
    assert_eq!(path, "Pictures/photo.jpg");
    assert_eq!(&bytes[..3], &[0xff, 0xd8, 0xff]);
    // GIMP saves only XCF; a PNG it opened is exported, or overwritten on request.
    let mut g = studio(Product::Gimp);
    g.path = "Pictures/photo.png".into();
    assert!(g.save_target().is_none());
    assert_eq!(g.overwrite_target().as_deref(), Some("Pictures/photo.png"));
    let effects = g.command(1, "save").unwrap();
    assert!(matches!(&effects[0], AppEffect::ListDirectory { .. }));
    let Some(Panel::Save { name, export, .. }) = &g.panel else {
        panic!("no save sheet");
    };
    assert_eq!((name.as_str(), *export), ("photo.xcf", false));
    assert!(g.command(1, "format:png").is_err(), "Save is XCF only");
}

#[test]
fn files_open_through_the_sheet_and_the_decoded_pixels() {
    let (mut s, effects) = Studio::launch(Product::Preview, "", 7);
    assert!(matches!(
        &effects[0],
        AppEffect::ListDirectory { window: 7, .. }
    ));
    s.listed(vec!["b.png".into(), "a.jpg".into(), "doc.txt".into()]);
    assert!(s.command(7, "open:doc.txt").is_err());
    let effects = s.command(7, "open:b.png").unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadImage { path, .. } if path == "Pictures/b.png"));
    assert!(s.image("Pictures/other.png", 1, 1, vec![0; 4]).is_err());
    s.image("Pictures/b.png", 2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255])
        .unwrap();
    assert_eq!(s.doc.as_ref().unwrap().pixel(1, 0), [4, 5, 6, 255]);
    assert_eq!(s.document_name(), "b.png");
    s.image_failed("Pictures/b.png", "nope");
    let mut t = Studio::new(Product::Gimp);
    t.loading = Some("x.png".into());
    t.image_failed("x.png", "not a PNG");
    assert!(t.status.as_deref().unwrap().contains("not a PNG"));
}

#[test]
fn text_is_typed_rasterised_and_stamped() {
    let mut s = studio(Product::Gimp);
    s.zoom = 100;
    s.command(1, "tool:text").unwrap();
    assert!(!s.accepts_text());
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 2, 3)
        .unwrap();
    assert!(s.accepts_text());
    s.text("Hi").unwrap();
    let effects = s.key(1, "Enter").unwrap();
    assert!(matches!(&effects[0], AppEffect::RasterText { text, size: 24, .. } if text == "Hi"));
    assert!(!s.accepts_text(), "waiting for glyphs");
    let mut alpha = vec![0u8; 3 * 2];
    alpha[0] = 255;
    alpha[4] = 128;
    s.text_rasterized(3, 2, alpha).unwrap();
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(2, 3), cw_raster::BLACK);
    assert_eq!(doc.pixel(3, 4), [127, 127, 127, 255]);
    assert!(s.text.is_none());
    assert!(s.text_rasterized(1, 1, vec![0]).is_err());
}

#[test]
fn clipboard_copies_and_pastes_as_a_layer() {
    let mut s = studio(Product::Pixelmator);
    s.doc.as_mut().unwrap().select(
        Mask::rect(40, 30, IRect::new(0, 0, 5, 4)),
        SelectMode::Replace,
    );
    let effects = s.command(1, "copy").unwrap();
    assert!(matches!(
        &effects[0],
        AppEffect::CopyImage {
            width: 5,
            height: 4,
            ..
        }
    ));
    let effects = s.command(1, "paste").unwrap();
    assert!(matches!(&effects[0], AppEffect::PasteImage { window: 1 }));
    s.image(CLIPBOARD_IMAGE, 1, 1, vec![9, 9, 9, 255]).unwrap();
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.pixel(0, 0), [9, 9, 9, 255]);
    assert_eq!(s.tool, Tool::Move);
    s.image_failed(CLIPBOARD_IMAGE, "The clipboard holds no picture");
    assert_eq!(s.status.as_deref(), Some("The clipboard holds no picture"));
}

#[test]
fn phone_looks_preview_and_bake() {
    let mut s = studio(Product::IosPhotos);
    s.doc = Some(Document::new(4, 4, Some([100, 100, 100, 255])).unwrap());
    assert!(s.command(1, "focus:hue").is_err(), "iOS has no Hue slider");
    s.command(1, "focus:saturation").unwrap();
    s.command(1, "set:brightness:100").unwrap();
    assert_eq!(s.param("brightness"), 100);
    // The dial moves the value by how far it is dragged.
    s.pointer(1, "dial:brightness", PointerPhase::Down, 100, 0)
        .unwrap();
    s.pointer(1, "dial:brightness", PointerPhase::Up, 130, 0)
        .unwrap();
    assert_eq!(s.param("brightness"), 90);
    // Adjustments preview only; the document is untouched until saved.
    assert_eq!(s.doc.as_ref().unwrap().pixel(0, 0), [100, 100, 100, 255]);
    let effects = look::done(&s, 1, &[]).unwrap();
    let AppEffect::WriteImage { path, rgba, .. } = &effects[0] else {
        panic!("not a write");
    };
    assert_eq!(path, "Pictures/photo.png");
    // Brightness 90/2 = 45: 100 + 155 * 0.45 = 169.75 -> 170.
    assert_eq!(&rgba[..4], &[170, 170, 170, 255]);
    // Google Photos saves a copy that does not collide with what is there.
    let mut g = studio(Product::GooglePhotos);
    g.command(1, "preset:vivid").unwrap();
    let effects = look::done(&g, 1, &["photo-edited.png".into()]).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::WriteImage { path, .. } if path == "Pictures/photo-edited 2.png")
    );
    // Markup only draws in the Markup tab.
    assert!(g
        .pointer(1, "canvas:40:30", PointerPhase::Down, 1, 1)
        .is_err());
    g.command(1, "tab:markup").unwrap();
    g.command(1, "tool:pen").unwrap();
    assert!(g
        .pointer(1, "canvas:40:30", PointerPhase::Down, 1, 1)
        .is_ok());
    g.command(1, "look:aspect:square").unwrap();
    assert_eq!(g.doc.as_ref().unwrap().width(), 30);
}

#[test]
fn studios_round_trip_through_snapshots() {
    for (product, _) in PRODUCTS {
        let mut s = studio(product);
        s.command(1, "undo").ok();
        s.gesture = Some(Gesture::Lasso {
            points: vec![(1, 2), (3, 4)],
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: Studio = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
    for kind in [
        "paint",
        "preview",
        "pixelmator",
        "gimp",
        "pinta",
        "sketchbook",
    ] {
        let (app, _) = crate::NativeApp::launch(kind, "", 1, 0).expect("launches");
        assert_eq!(app.kind(), kind);
    }
}

/// A drag through `points` on a 40x30 canvas shown at 100% (view = image pixels).
fn drag(s: &mut Studio, points: &[(i32, i32)]) {
    let (first, last) = (points[0], points[points.len() - 1]);
    s.pointer(1, "canvas:40:30", PointerPhase::Down, first.0, first.1)
        .unwrap();
    for p in &points[1..] {
        s.pointer(1, "canvas:40:30", PointerPhase::Move, p.0, p.1)
            .unwrap();
    }
    s.pointer(1, "canvas:40:30", PointerPhase::Up, last.0, last.1)
        .unwrap();
}
/// A drag through the centres of image pixels: the 40x30 image at 200% in an 80x60
/// view, where view pixel `2x + 1` is the centre of image pixel `x`.
fn drag_px(s: &mut Studio, pixels: &[(i32, i32)]) {
    s.zoom = 200;
    let at = |(x, y): (i32, i32)| (2 * x + 1, 2 * y + 1);
    let (first, last) = (at(pixels[0]), at(pixels[pixels.len() - 1]));
    s.pointer(1, "canvas:80:60", PointerPhase::Down, first.0, first.1)
        .unwrap();
    for p in &pixels[1..] {
        let (x, y) = at(*p);
        s.pointer(1, "canvas:80:60", PointerPhase::Move, x, y)
            .unwrap();
    }
    s.pointer(1, "canvas:80:60", PointerPhase::Up, last.0, last.1)
        .unwrap();
}
fn at100(product: Product) -> Studio {
    let mut s = studio(product);
    s.zoom = 100;
    s
}
fn painted(s: &Studio, theme: DesktopTheme, pointer: Option<(i32, i32)>) -> cw_scene::Scene {
    let mut p = Painter::themed(theme, 1100, 760, 0);
    let mut e = env(theme, 1100, 760);
    e.pointer = pointer;
    s.render(&mut p, &e);
    p.scene
}
/// Every primitive the render painted, as text.
fn painted_text(s: &Studio, theme: DesktopTheme, pointer: Option<(i32, i32)>) -> String {
    painted(s, theme, pointer)
        .nodes
        .iter()
        .map(|n| format!("{:?}", n.primitive))
        .collect::<Vec<_>>()
        .join("\n")
}
/// The canvas's painted rectangle and target.
fn canvas_node(s: &Studio, theme: DesktopTheme) -> (cw_scene::Rect, String) {
    let mut p = Painter::themed(theme, 1100, 760, 0);
    s.render(&mut p, &env(theme, 1100, 760));
    p.scene
        .nodes
        .iter()
        .find_map(|n| {
            let t = n.interaction.as_deref()?;
            t.contains(":canvas:").then(|| (n.bounds, t.to_owned()))
        })
        .unwrap()
}

#[test]
fn hovering_the_canvas_reports_the_pointer_and_outlines_the_brush() {
    for (product, theme, expect) in [
        (Product::Paint, DesktopTheme::Windows, "12, 7px"),
        (Product::Gimp, DesktopTheme::Ubuntu, "12, 7"),
        (Product::Pinta, DesktopTheme::Ubuntu, "12, 7"),
    ] {
        let mut s = at100(product);
        s.tool = Tool::Brush;
        s.size = 10;
        let (r, target) = canvas_node(&s, theme);
        assert!(s.hovers(&target));
        assert!(!s.hovers(&s.target("slider:size:100")));
        let (ox, oy) = s.origin(r.width, r.height);
        assert!(s.hover(&target, ox + 12, oy + 7), "the view changes");
        assert!(!s.hover(&target, ox + 12, oy + 7), "same place, no change");
        let pointer = (r.x + ox + 12, r.y + oy + 7);
        let text = painted_text(&s, theme, Some(pointer));
        assert!(text.contains(expect), "{product:?} shows {expect}");
        // The brush outline: a ring of the brush's radius (10 px at 100%) around the
        // pointer, only while it is over the canvas.
        let ring = cw_scene::Rect::new(pointer.0 - 5, pointer.1 - 5, 10, 10);
        let rings =
            |scene: &cw_scene::Scene| scene.nodes.iter().filter(|n| n.bounds == ring).count();
        assert_eq!(
            rings(&painted(&s, theme, Some(pointer))),
            1,
            "{product:?} outlines the brush"
        );
        assert_eq!(rings(&painted(&s, theme, Some((2, 2)))), 0);
        // Off the canvas the readout is blank.
        let away = painted_text(&s, theme, Some((2, 2)));
        assert!(!away.contains(expect), "{product:?} blanks the readout");
    }
    // Nothing is open: nothing to hover.
    let empty = Studio::new(Product::Gimp);
    assert!(!empty.hovers("gimp:canvas:40:30"));
}

#[test]
fn filter_dialogs_preview_live_and_cancel_restores_exactly() {
    for product in [Product::Gimp, Product::Pinta, Product::Pixelmator] {
        let mut s = at100(product);
        s.doc.as_mut().unwrap().select(
            Mask::rect(40, 30, IRect::new(0, 0, 20, 30)),
            SelectMode::Replace,
        );
        s.doc
            .as_mut()
            .unwrap()
            .fill(10, 10, [0, 0, 0, 255], 0, false);
        let before = s.doc.clone().unwrap();
        s.command(1, "dialog:gaussian-blur").unwrap();
        s.command(1, "set:radius:4").unwrap();
        let preview = s.preview_layer().expect("a filter previews");
        // Blurred across the selection's edge inside it; untouched outside.
        assert_ne!(
            preview.get(19, 10),
            before.active_layer().canvas.get(19, 10)
        );
        assert_eq!(preview.get(21, 10), cw_raster::WHITE);
        assert_eq!(
            s.doc.as_ref().unwrap(),
            &before,
            "the document is untouched"
        );
        if product == Product::Gimp {
            s.command(1, "preview-toggle").unwrap();
            assert!(s.preview_layer().is_none(), "Preview unticked");
            s.command(1, "preview-toggle").unwrap();
        }
        s.command(1, "cancel").unwrap();
        assert!(s.preview_layer().is_none());
        assert_eq!(s.doc.as_ref().unwrap(), &before, "Cancel restores exactly");
        // OK commits exactly what was previewed, as one step.
        s.command(1, "dialog:gaussian-blur").unwrap();
        s.command(1, "set:radius:4").unwrap();
        let preview = s.preview_layer().unwrap();
        s.command(1, "apply").unwrap();
        let doc = s.doc.as_ref().unwrap();
        assert_eq!(doc.active_layer().canvas, preview);
        assert_eq!(doc.undo_label(), Some("Gaussian Blur"));
    }
}

#[test]
fn the_move_tool_shows_the_layer_moving_during_the_drag() {
    let mut s = at100(Product::Pinta);
    s.doc
        .as_mut()
        .unwrap()
        .fill(0, 0, [0, 0, 255, 255], 0, false);
    s.doc.as_mut().unwrap().select(
        Mask::rect(40, 30, IRect::new(2, 2, 3, 3)),
        SelectMode::Replace,
    );
    s.doc
        .as_mut()
        .unwrap()
        .fill(3, 3, [255, 0, 0, 255], 0, false);
    s.doc.as_mut().unwrap().select_none();
    let before = s.doc.clone().unwrap();
    s.command(1, "tool:move").unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 3, 3)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Move, 13, 8)
        .unwrap();
    let shown = s.preview_layer().expect("the moving layer shows");
    assert_eq!(shown.get(13, 8), [255, 0, 0, 255]);
    assert_eq!(
        shown.get(3, 3),
        [0, 0, 0, 0],
        "the layer moved away from here"
    );
    assert_eq!(s.doc.as_ref().unwrap(), &before, "not moved until release");
    s.pointer(1, "canvas:40:30", PointerPhase::Up, 13, 8)
        .unwrap();
    assert_eq!(s.doc.as_ref().unwrap().active_layer().canvas, shown);
    assert!(s.preview_layer().is_none());
}

#[test]
fn gradients_preview_while_dragged_and_paint_on_release() {
    let mut s = at100(Product::Gimp);
    s.command(1, "tool:gradient").unwrap();
    s.command(1, "reset-colors").unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 0, 5)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Move, 40, 5)
        .unwrap();
    let shown = s
        .preview_layer()
        .expect("the gradient shows while dragging");
    assert!(shown.get(1, 0)[0] < 20 && shown.get(38, 29)[0] > 235);
    assert_eq!(s.doc.as_ref().unwrap().pixel(1, 0), cw_raster::WHITE);
    s.pointer(1, "canvas:40:30", PointerPhase::Up, 40, 5)
        .unwrap();
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.active_layer().canvas, shown);
    assert_eq!(doc.undo_label(), Some("Gradient"));
    // FG to Transparent, radial, triangular, reversed: each changes the result.
    for command in [
        "gradient-colors:fg-transparent",
        "gradient-shape:radial",
        "gradient-repeat:triangular",
        "gradient-reverse",
    ] {
        s.command(1, command).unwrap();
    }
    assert!(s.gradient.transparent && s.gradient.reverse);
    s.pointer(1, "canvas:40:30", PointerPhase::Down, 20, 15)
        .unwrap();
    s.pointer(1, "canvas:40:30", PointerPhase::Move, 30, 15)
        .unwrap();
    let radial = s.preview_layer().unwrap();
    // Reversed FG to Transparent: nearly clear at the centre, solid 10 px out.
    let before = s.doc.as_ref().unwrap().pixel(20, 15);
    let centre_px = radial.get(20, 15);
    assert!(
        centre_px[0].abs_diff(before[0]) < 30,
        "{centre_px:?} vs {before:?}"
    );
    assert!(radial.get(29, 15)[0] < 20, "{:?}", radial.get(29, 15));
    s.pointer(1, "canvas:40:30", PointerPhase::Cancel, 30, 15)
        .unwrap();
    assert!(s.command(1, "gradient-shape:spiral").is_err());
}

#[test]
fn a_clone_source_is_set_by_modifier_click_and_strokes_copy_from_it() {
    let red = [255, 0, 0, 255];
    let setup = |product| {
        let mut s = at100(product);
        let d = s.doc.as_mut().unwrap();
        d.select(
            Mask::rect(40, 30, IRect::new(5, 5, 3, 3)),
            SelectMode::Replace,
        );
        d.fill(6, 6, [255, 0, 0, 255], 0, false);
        d.select_none();
        s.command(1, "tool:clone").unwrap();
        s.size = 1;
        s
    };
    let mut s = setup(Product::Gimp);
    // No source yet: the brush refuses to paint and says how to set one.
    drag_px(&mut s, &[(20, 10), (22, 10)]);
    assert_eq!(s.doc.as_ref().unwrap().pixel(20, 10), cw_raster::WHITE);
    assert!(s.status.as_deref().unwrap().contains("Ctrl-click"));
    // Ctrl-click sets it; the click paints nothing.
    s.modifiers = MOD_CTRL;
    drag_px(&mut s, &[(6, 6)]);
    s.modifiers = 0;
    assert_eq!(s.retouch.source, Some((6, 6)));
    drag_px(&mut s, &[(20, 10), (21, 10), (22, 10)]);
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(20, 10), red, "(6, 6)");
    assert_eq!(doc.pixel(21, 10), red, "(7, 6)");
    assert_eq!(
        doc.pixel(22, 10),
        cw_raster::WHITE,
        "(8, 6) is outside the patch"
    );
    assert_eq!(doc.undo_label(), Some("Clone"));
    // Non-aligned (GIMP's default): the next stroke starts from the source again.
    drag_px(&mut s, &[(30, 20)]);
    assert_eq!(s.doc.as_ref().unwrap().pixel(30, 20), red);
    // Aligned: the first stroke's offset is kept, so a stroke elsewhere samples
    // elsewhere.
    s.command(1, "aligned:on").unwrap();
    drag_px(&mut s, &[(20, 20)]);
    drag_px(&mut s, &[(26, 26)]);
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(20, 20), red);
    assert_eq!(
        doc.pixel(26, 26),
        cw_raster::WHITE,
        "(12, 12) through the offset"
    );
    // Pixelmator sets its source with Option, not Ctrl.
    let mut m = setup(Product::Pixelmator);
    m.modifiers = MOD_CTRL;
    drag_px(&mut m, &[(6, 6)]);
    assert_eq!(m.retouch.source, None);
    m.modifiers = MOD_ALT;
    drag_px(&mut m, &[(6, 6)]);
    assert_eq!(m.retouch.source, Some((6, 6)));
    // Pinta's Clone Stamp: Ctrl, as GIMP.
    let mut p = setup(Product::Pinta);
    p.modifiers = MOD_CTRL;
    drag_px(&mut p, &[(5, 5)]);
    p.modifiers = 0;
    drag_px(&mut p, &[(30, 3)]);
    assert_eq!(p.doc.as_ref().unwrap().pixel(30, 3), red);
}

#[test]
fn heal_and_repair_retouch_through_the_ui() {
    // GIMP's Heal: texture from the source, colour from where it lands.
    let mut s = at100(Product::Gimp);
    {
        let d = s.doc.as_mut().unwrap();
        d.select(
            Mask::rect(40, 30, IRect::new(20, 0, 20, 30)),
            SelectMode::Replace,
        );
        d.fill(30, 10, [100, 100, 100, 255], 0, false);
        d.select_none();
    }
    s.command(1, "tool:heal").unwrap();
    s.size = 5;
    s.modifiers = MOD_CTRL;
    drag(&mut s, &[(8, 15)]);
    s.modifiers = 0;
    drag(&mut s, &[(30, 15)]);
    let doc = s.doc.as_ref().unwrap();
    // White healed onto grey becomes grey, not white.
    assert_eq!(doc.pixel(30, 15), [100, 100, 100, 255]);
    assert_eq!(doc.undo_label(), Some("Heal"));
    // Pixelmator's Repair: the blemish is gone on release.
    let mut m = at100(Product::Pixelmator);
    m.doc.as_mut().unwrap().select(
        Mask::rect(40, 30, IRect::new(18, 12, 3, 3)),
        SelectMode::Replace,
    );
    m.doc
        .as_mut()
        .unwrap()
        .fill(19, 13, [0, 0, 0, 255], 0, false);
    m.doc.as_mut().unwrap().select_none();
    m.command(1, "tool:repair").unwrap();
    m.size = 8;
    m.pointer(1, "canvas:40:30", PointerPhase::Down, 19, 13)
        .unwrap();
    assert!(m.doc.as_ref().unwrap().repair_mask().is_some());
    assert_eq!(m.doc.as_ref().unwrap().pixel(19, 13), [0, 0, 0, 255]);
    m.pointer(1, "canvas:40:30", PointerPhase::Up, 19, 13)
        .unwrap();
    assert_eq!(m.doc.as_ref().unwrap().pixel(19, 13), cw_raster::WHITE);
    assert_eq!(m.doc.as_ref().unwrap().undo_label(), Some("Repair"));
}

#[test]
fn gimp_paths_are_drawn_closed_and_stroked_filled_or_selected() {
    let mut s = at100(Product::Gimp);
    s.command(1, "tool:paths").unwrap();
    assert!(s.command(1, "path:fill").is_err(), "no path yet");
    for p in [(5, 5), (30, 5), (30, 25), (5, 25)] {
        drag(&mut s, &[p]);
    }
    assert_eq!(s.path_edit.as_ref().unwrap().anchors.len(), 4);
    // A plain click on the first anchor only takes it; Ctrl-click closes the path.
    drag(&mut s, &[(5, 5)]);
    assert!(!s.path_edit.as_ref().unwrap().closed);
    s.modifiers = MOD_CTRL;
    drag(&mut s, &[(5, 5)]);
    s.modifiers = 0;
    assert!(s.path_edit.as_ref().unwrap().closed);
    // Dragging an anchor moves it.
    drag(&mut s, &[(30, 25), (32, 27)]);
    // At 100% a view pixel's top-left corner is the image point it names.
    assert_eq!(
        s.path_edit.as_ref().unwrap().anchors[2].point,
        (32 * 16, 27 * 16)
    );
    drag(&mut s, &[(32, 27), (30, 25)]);
    s.command(1, "path:select").unwrap();
    let sel = s.doc.as_ref().unwrap().selection().unwrap().clone();
    assert_eq!(sel.get(15, 15), 255);
    assert_eq!(sel.get(2, 2), 0);
    s.command(1, "select-none").unwrap();
    s.command(1, "color:ff0000").unwrap();
    s.command(1, "path:fill").unwrap();
    assert_eq!(s.doc.as_ref().unwrap().pixel(15, 15), [255, 0, 0, 255]);
    assert_eq!(s.doc.as_ref().unwrap().undo_label(), Some("Fill Path"));
    s.command(1, "undo").unwrap();
    s.command(1, "color:0000ff").unwrap();
    s.command(1, "path:stroke").unwrap();
    s.command(1, "set:line-width:2").unwrap();
    s.command(1, "apply").unwrap();
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.pixel(15, 5), [0, 0, 255, 255], "on the top edge");
    assert_eq!(doc.pixel(15, 15), cw_raster::WHITE, "inside is not filled");
    assert_eq!(doc.undo_label(), Some("Stroke Path"));
    // A new anchor dragged out pulls smooth handles.
    s.command(1, "path:delete").unwrap();
    drag(&mut s, &[(10, 10), (14, 10)]);
    let a = s.path_edit.as_ref().unwrap().anchors[0];
    assert_eq!((a.cout, a.cin), ((14 * 16, 160), (6 * 16, 160)));
    assert!(studio(Product::Pinta).command(1, "path:fill").is_err());
}

#[test]
fn paint_curves_bend_twice_and_pinta_curves_take_control_points() {
    let mut s = at100(Product::Paint);
    s.command(1, "shape:curve").unwrap();
    s.size = 1;
    drag(&mut s, &[(2, 15), (37, 15)]);
    assert!(s.curve.is_some());
    assert!(!s.doc.as_ref().unwrap().can_undo(), "not drawn yet");
    drag(&mut s, &[(12, 2)]);
    assert!(s.curve.is_some(), "one bend placed");
    drag(&mut s, &[(27, 28)]);
    assert!(s.curve.is_none(), "the second bend draws it");
    let doc = s.doc.as_ref().unwrap();
    let dark = |x: i32, ys: std::ops::Range<i32>| ys.into_iter().any(|y| doc.pixel(x, y)[0] < 128);
    assert!(dark(12, 5..12), "bent up on the left");
    assert!(dark(27, 18..25), "bent down on the right");
    assert!(!dark(20, 0..14) || !dark(20, 17..30), "an S, not a line");

    let mut p = at100(Product::Pinta);
    p.command(1, "shape:line").unwrap();
    p.size = 1;
    drag(&mut p, &[(2, 15), (37, 15)]);
    assert_eq!(p.curve.as_ref().unwrap().points.len(), 2);
    // Press on the line and drag: a new control point, pulled up.
    drag(&mut p, &[(20, 15), (20, 5)]);
    assert_eq!(p.curve.as_ref().unwrap().points.len(), 3);
    p.key(1, "Enter").unwrap();
    assert!(p.curve.is_none());
    let doc = p.doc.as_ref().unwrap();
    assert!(
        (3..8).any(|y| doc.pixel(20, y)[0] < 128),
        "through the new point"
    );
    // Freeform: a closed shape through the drag, filled with the secondary colour.
    p.command(1, "shape:freeform").unwrap();
    p.command(1, "fill-style:fill").unwrap();
    p.secondary = [0, 200, 0, 255];
    drag(&mut p, &[(5, 20), (15, 20), (15, 28), (5, 28)]);
    assert_eq!(p.doc.as_ref().unwrap().pixel(10, 24), [0, 200, 0, 255]);
}

#[test]
fn every_format_is_written_and_read_back() {
    // GIMP: two layers saved as XCF reopen as the same layers.
    let mut g = at100(Product::Gimp);
    g.command(1, "layer:new").unwrap();
    g.command(1, "color:ff0000").unwrap();
    g.command(1, "tool:pencil").unwrap();
    drag(&mut g, &[(3, 3), (9, 3)]);
    g.command(1, "layer:blend:multiply").unwrap();
    g.command(1, "save").unwrap();
    assert!(matches!(&g.panel, Some(Panel::Save { name, .. }) if name == "Untitled.xcf"));
    g.command(1, "xcf-compression").unwrap();
    let effects = g.command(1, "save-confirm").unwrap();
    let AppEffect::WriteBytes { path, bytes, .. } = &effects[0] else {
        panic!("not a byte write");
    };
    assert_eq!(path, "Pictures/Untitled.xcf");
    assert_eq!(&bytes[..9], b"gimp xcf ");
    g.bytes_saved(path, Ok(()));
    assert_eq!(g.save_target().as_deref(), Some("Pictures/Untitled.xcf"));
    let (mut again, effects) = Studio::launch(Product::Gimp, "Pictures/Untitled.xcf", 2);
    assert!(matches!(&effects[0], AppEffect::ReadBytes { .. }));
    again.bytes_loaded("Pictures/Untitled.xcf", Ok(bytes.clone()));
    let back = again.doc.as_ref().unwrap();
    assert_eq!(back.layers(), g.doc.as_ref().unwrap().layers());
    // Export As JPEG asks for quality and subsampling, then writes a JPEG.
    g.command(1, "export").unwrap();
    g.command(1, "format:jpg").unwrap();
    assert!(g.command(1, "save-confirm").unwrap().is_empty());
    assert!(matches!(&g.panel, Some(Panel::Dialog { id, .. }) if id == "jpeg"));
    g.command(1, "set:quality:75").unwrap();
    g.command(1, "set:subsampling:1").unwrap();
    let effects = g.command(1, "apply").unwrap();
    let AppEffect::WriteBytes { path, bytes, .. } = &effects[0] else {
        panic!("not a byte write");
    };
    assert_eq!(path, "Pictures/Untitled.jpg");
    assert_eq!(&bytes[..2], &[0xff, 0xd8]);
    assert_eq!((g.quality(), g.subsampling), (75, Subsampling::Quartered));
    // Paint: Save as ▸ BMP picture, and the bitmap opens again.
    let mut p = at100(Product::Paint);
    p.command(1, "color:00ff00").unwrap();
    p.command(1, "tool:pencil").unwrap();
    drag(&mut p, &[(4, 4)]);
    p.command(1, "save-as:bmp").unwrap();
    let effects = p.command(1, "save-confirm").unwrap();
    let AppEffect::WriteBytes { path, bytes, .. } = &effects[0] else {
        panic!("not a byte write");
    };
    assert_eq!(path, "Pictures/Untitled.bmp");
    let (mut q, _) = Studio::launch(Product::Paint, "Pictures/Untitled.bmp", 3);
    q.bytes_loaded(path, Ok(bytes.clone()));
    assert_eq!(
        q.doc.as_ref().unwrap().composite(),
        p.doc.as_ref().unwrap().composite()
    );
    // Pinta's JPEG Quality dialog; Preview's quality slider lives in its sheet.
    let mut pinta = at100(Product::Pinta);
    pinta.command(1, "save-as:jpg").unwrap();
    pinta.command(1, "save-confirm").unwrap();
    assert!(matches!(&pinta.panel, Some(Panel::Dialog { id, .. }) if id == "jpeg-quality"));
    assert_eq!(pinta.param("quality"), 85);
    pinta.command(1, "cancel").unwrap();
    assert!(pinta.pending.is_none());
    let mut preview = at100(Product::Preview);
    preview.command(1, "save-as:jpg").unwrap();
    preview.command(1, "set:jpeg-quality:40").unwrap();
    let effects = preview.command(1, "save-confirm").unwrap();
    assert!(matches!(&effects[0], AppEffect::WriteBytes { path, .. } if path.ends_with(".jpg")));
    assert!(
        preview.command(1, "format:bmp").is_err(),
        "Preview writes no bitmaps"
    );
    // What cannot be read says so.
    let (mut bad, _) = Studio::launch(Product::Gimp, "Pictures/bad.xcf", 4);
    bad.bytes_loaded("Pictures/bad.xcf", Ok(b"not an xcf".to_vec()));
    assert!(bad.doc.is_none());
    assert!(bad
        .status
        .as_deref()
        .unwrap()
        .contains("could not be opened"));
    assert!(Product::Gimp.opens("a.xcf") && !Product::Pinta.opens("a.xcf"));
    assert!(Product::Paint.opens("a.bmp") && !Product::IosPhotos.opens("a.bmp"));
}
