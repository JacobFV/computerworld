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
    assert_eq!(preview_adjustments(s.panel.as_ref()).len(), 1);
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
    // A JPEG original is never overwritten with PNG bytes.
    s.path = "Pictures/photo.jpg".into();
    assert!(s.save_target().is_none());
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
