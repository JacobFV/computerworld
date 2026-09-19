//! FreeCAD's main window in the "FreeCAD Light" theme.
use super::commands::{command, toolbar, MENUS};
use super::layout::{layout, Layout, ROW_H, TAB_H};
use super::props::Kind;
use super::tasks::Param;
use super::*;
use crate::desktop_scene::shared::Align;
use cw_cad::document::Status;
use cw_cad::math::fmt_num;
use cw_cad::sketch::{ConstraintType as T, SolveStatus};
use cw_scene::{Color, Lang, Rect, Style};

pub const BG: Color = Color::rgb(240, 240, 240);
pub const EDGE: Color = Color::rgb(171, 171, 171);
pub const INK: Color = Color::rgb(0, 0, 0);
pub const DIM: Color = Color::rgb(100, 100, 100);
pub const WHITE: Color = Color::rgb(255, 255, 255);
pub const ACCENT: Color = Color::rgb(74, 165, 255);
pub const SELECTED: Color = Color(74, 165, 255, 110);
pub const HOVER: Color = Color(74, 165, 255, 50);
pub const ERROR: Color = Color::rgb(224, 49, 49);
pub const WARN: Color = Color::rgb(230, 119, 0);
pub const OK_GREEN: Color = Color::rgb(47, 158, 68);

fn over(pointer: Option<(i32, i32)>, r: Rect) -> bool {
    pointer.is_some_and(|(x, y)| r.contains(x, y))
}

pub fn render(cad: &Cad, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let l = layout(env.theme, w, h, cad.show_report);
    let pointer = env.pointer;
    p.scene.background = BG;
    p.box_(Rect::new(0, 0, w, h), BG, 0);
    if let Some(m) = l.menu {
        menu_bar(cad, p, m, pointer);
    }
    toolbar_row(cad, p, &l, pointer);
    combo_view(cad, p, &l, pointer);
    // Splitter between the dock and the view.
    p.vline(
        l.combo.x + l.combo.width as i32 + 1,
        l.combo.y,
        l.combo.height,
        EDGE,
    );
    super::view3d::draw(cad, p, &l, pointer);
    if let Some(r) = l.report {
        report_view(cad, p, r);
    }
    status_bar(cad, p, &l, pointer);
    dialogs(cad, p, &l, env);
    popups(cad, p, &l, pointer);
}

// ---------------------------------------------------------------------------------
// Menu bar and toolbars.

fn menu_bar(cad: &Cad, p: &mut Painter, r: Rect, pointer: Option<(i32, i32)>) {
    p.box_(r, BG, 0);
    p.hline(r.x, r.y + r.height as i32 - 1, r.width, Color(0, 0, 0, 30));
    let mut x = r.x + 4;
    for (name, _) in MENUS {
        let tw = p.measure(name, 12, false) + 16;
        let item = Rect::new(x, r.y + 1, tw, r.height - 2);
        let open = cad.menu.as_deref() == Some(*name);
        if open || over(pointer, item) {
            p.box_(item, if open { SELECTED } else { HOVER }, 0);
        }
        p.label(x, r.y + 3, tw, name, 12, INK, false, Align::Center);
        p.region(item, &format!("freecad:menu:{name}"), name);
        x += tw as i32;
    }
}

/// Where each toolbar item goes: (id, rect); the ones that do not fit go to the
/// overflow menu.
pub fn toolbar_items(
    cad: &Cad,
    p: &Painter,
    l: &Layout,
) -> (Vec<(&'static str, Rect)>, Vec<&'static str>) {
    let sketching = cad.sketch_edit().is_some();
    let mut x = l.toolbar.x + 6;
    let y = l.toolbar.y + 3;
    let limit = l.toolbar.x + l.toolbar.width as i32 - 26;
    let mut placed = Vec::new();
    let mut overflow = Vec::new();
    for id in toolbar(sketching) {
        let w = match id {
            "|" => 9,
            "workbench" => p.measure(workbench_name(cad), 12, false) + 44,
            _ => 27,
        };
        if x + w as i32 > limit {
            if id != "|" && id != "workbench" {
                overflow.push(id);
            }
            continue;
        }
        placed.push((id, Rect::new(x, y, w, 26)));
        x += w as i32 + 1;
    }
    (placed, overflow)
}
fn workbench_name(cad: &Cad) -> &'static str {
    if cad.sketch_edit().is_some() {
        "Sketcher"
    } else {
        "Part Design"
    }
}

fn toolbar_row(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let r = l.toolbar;
    p.box_(r, BG, 0);
    p.hline(r.x, r.y + r.height as i32 - 1, r.width, Color(0, 0, 0, 30));
    let (placed, overflow) = toolbar_items(cad, p, l);
    let active_tool = cad
        .sketch_edit()
        .and_then(|e| e.tool.map(|t| t.command()).or(e.pending.as_deref()));
    for (id, b) in placed {
        match id {
            "|" => p.vline(b.x + 4, b.y + 2, b.height - 4, EDGE),
            "workbench" => {
                let open = cad.menu.as_deref() == Some("workbench");
                p.border(b, WHITE, 3, if open { ACCENT } else { EDGE });
                super::icons::draw(
                    p,
                    if cad.sketch_edit().is_some() {
                        "Sketcher_Workbench"
                    } else {
                        "PartDesign_Workbench"
                    },
                    b.x + 4,
                    b.y + 5,
                    16,
                    true,
                );
                p.left(
                    b.x + 23,
                    b.y + 5,
                    b.width - 30,
                    workbench_name(cad),
                    12,
                    INK,
                );
                p.symbol("chevron-down", b.x + b.width as i32 - 13, b.y + 9, 9, DIM);
                p.region(b, "freecad:workbench", "Switch workbench");
            }
            _ => {
                let available = cad.available(id);
                let on = active_tool == Some(id)
                    || (id == "Sketcher_ToggleConstruction"
                        && cad.sketch_edit().is_some_and(|e| e.construction))
                    || (id == "Std_Measure" && matches!(cad.task, Some(Task::Measure)));
                if on {
                    p.border(b, Color::rgb(216, 216, 216), 3, EDGE);
                } else if available.is_ok() && over(pointer, b) {
                    p.border(b, Color::TRANSPARENT, 3, EDGE);
                }
                super::icons::draw(p, id, b.x + 3, b.y + 3, 20, available.is_ok());
                let label = command(id).map(|c| c.label).unwrap_or(id);
                match available {
                    Ok(()) => p.region(b, &format!("freecad:cmd:{id}"), label),
                    Err(why) => {
                        p.box_(b, Color::TRANSPARENT, 0);
                        p.disabled(&format!("{label}: {why}"));
                    }
                }
            }
        }
    }
    if !overflow.is_empty() {
        let b = Rect::new(r.x + r.width as i32 - 22, r.y + 5, 18, 22);
        if over(pointer, b) || cad.menu.as_deref() == Some("overflow") {
            p.border(b, Color::TRANSPARENT, 3, EDGE);
        }
        p.label(b.x, b.y + 2, b.width, "»", 14, INK, true, Align::Center);
        p.region(b, "freecad:overflow", "More tools");
    }
}

// ---------------------------------------------------------------------------------
// Combo view.

fn combo_view(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let c = l.combo;
    p.box_(c, BG, 0);
    // Dock title.
    p.strong(c.x + 6, c.y + 3, c.width - 12, "Combo View", 12, INK);
    let tabs_y = c.y + 22;
    let mut x = c.x + 4;
    for (tab, label) in [(ComboTab::Model, "Model"), (ComboTab::Tasks, "Tasks")] {
        let tw = p.measure(label, 12, false) + 24;
        let r = Rect::new(x, tabs_y, tw, TAB_H);
        let on = cad.combo == tab;
        p.border(
            r,
            if on { WHITE } else { Color::rgb(221, 221, 221) },
            2,
            EDGE,
        );
        if on {
            p.hline(r.x + 1, r.y + r.height as i32 - 1, r.width - 2, WHITE);
        }
        p.label(
            r.x,
            r.y + 5,
            tw,
            label,
            12,
            if on { INK } else { DIM },
            on,
            Align::Center,
        );
        p.region(r, &format!("freecad:tab:{}", label.to_lowercase()), label);
        x += tw as i32 + 2;
    }
    let body = Rect::new(
        c.x + 2,
        tabs_y + TAB_H as i32 - 1,
        c.width - 4,
        c.height.saturating_sub(TAB_H + 24),
    );
    p.border(body, WHITE, 0, EDGE);
    match cad.combo {
        ComboTab::Model => {
            let tree_h = body.height * 55 / 100;
            let tree = Rect::new(body.x + 1, body.y + 1, body.width - 2, tree_h);
            model_tree(cad, p, tree, pointer);
            let props = Rect::new(
                body.x + 1,
                body.y + tree_h as i32 + 4,
                body.width - 2,
                body.height - tree_h - 5,
            );
            p.hline(body.x, body.y + tree_h as i32 + 2, body.width, EDGE);
            property_view(cad, p, props, pointer);
        }
        ComboTab::Tasks => task_panel(cad, p, body, pointer),
    }
}

struct TreeRow {
    depth: u32,
    name: String,
    label: String,
    icon: &'static str,
    expandable: bool,
    expanded: bool,
    eye: bool,
}

fn tree_rows(cad: &Cad) -> Vec<TreeRow> {
    let mut rows = vec![TreeRow {
        depth: 0,
        name: "document".into(),
        label: cad.doc.label.clone(),
        icon: "Document",
        expandable: false,
        expanded: true,
        eye: false,
    }];
    let icon_of = |f: &Feature| -> &'static str { f.type_id() };
    let used: BTreeSet<String> = cad
        .doc
        .objects
        .iter()
        .filter_map(|o| o.feature.profile().map(str::to_owned))
        .collect();
    let in_body: BTreeSet<String> = cad
        .doc
        .objects
        .iter()
        .flat_map(|o| match &o.feature {
            Feature::Body { group, .. } => group.clone(),
            _ => vec![],
        })
        .collect();
    for o in &cad.doc.objects {
        if in_body.contains(&o.name) {
            continue;
        }
        let expanded = cad.expanded.contains(&o.name);
        let is_body = matches!(o.feature, Feature::Body { .. });
        rows.push(TreeRow {
            depth: 1,
            name: o.name.clone(),
            label: o.label.clone(),
            icon: icon_of(&o.feature),
            expandable: is_body,
            expanded,
            eye: true,
        });
        let Feature::Body { group, .. } = &o.feature else {
            continue;
        };
        if !expanded {
            continue;
        }
        let origin = format!("{}:Origin", o.name);
        let origin_open = cad.expanded.contains(&origin);
        rows.push(TreeRow {
            depth: 2,
            name: origin.clone(),
            label: "Origin".into(),
            icon: "App::Origin",
            expandable: true,
            expanded: origin_open,
            eye: false,
        });
        if origin_open {
            for n in [
                "X_Axis", "Y_Axis", "Z_Axis", "XY_Plane", "XZ_Plane", "YZ_Plane",
            ] {
                rows.push(TreeRow {
                    depth: 3,
                    name: format!("origin:{n}"),
                    label: n.into(),
                    icon: if n.ends_with("Plane") {
                        "App::Plane"
                    } else {
                        "App::Line"
                    },
                    expandable: false,
                    expanded: false,
                    eye: false,
                });
            }
        }
        for g in group {
            if used.contains(g) {
                continue;
            }
            let Some(child) = cad.doc.get(g) else {
                continue;
            };
            let profile = child.feature.profile().map(str::to_owned);
            let open = cad.expanded.contains(g);
            rows.push(TreeRow {
                depth: 2,
                name: g.clone(),
                label: child.label.clone(),
                icon: icon_of(&child.feature),
                expandable: profile.is_some(),
                expanded: open,
                eye: true,
            });
            if let (Some(prof), true) = (profile, open) {
                if let Some(s) = cad.doc.get(&prof) {
                    rows.push(TreeRow {
                        depth: 3,
                        name: prof.clone(),
                        label: s.label.clone(),
                        icon: "Sketcher::SketchObject",
                        expandable: false,
                        expanded: false,
                        eye: true,
                    });
                }
            }
        }
    }
    rows
}

fn model_tree(cad: &Cad, p: &mut Painter, r: Rect, pointer: Option<(i32, i32)>) {
    p.box_(r, WHITE, 0);
    let model = cad.model();
    let tip_of = |name: &str| -> bool {
        cad.doc
            .body_of(name)
            .and_then(|b| match &cad.doc.get(b)?.feature {
                Feature::Body { tip, .. } => tip.clone(),
                _ => None,
            })
            == Some(name.to_owned())
    };
    let mark = p.scene.nodes.len();
    let mut y = r.y + 2;
    for row in tree_rows(cad) {
        if y + ROW_H as i32 > r.y + r.height as i32 {
            break;
        }
        let rr = Rect::new(r.x, y, r.width, ROW_H);
        // A feature's row lights when it, or one of its faces or edges, is selected.
        let selected = cad.selection.iter().any(|s| s.object == row.name)
            || matches!(&cad.task, Some(Task::PickPlane { plane, .. }) if row.name == format!("origin:{plane}"));
        let editing = cad.sketch_edit().is_some_and(|e| e.name == row.name)
            || matches!(&cad.task, Some(Task::Feature(f)) if f.name == row.name);
        if selected {
            p.box_(rr, SELECTED, 0);
        } else if editing {
            p.box_(rr, Color(33, 151, 255, 60), 0);
        } else if over(pointer, rr) {
            p.box_(rr, HOVER, 0);
        }
        let x = r.x + 4 + row.depth as i32 * 16;
        if row.expandable {
            let t = Rect::new(x - 14, y + 3, 14, 16);
            p.symbol(
                if row.expanded {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                t.x + 2,
                t.y + 3,
                10,
                DIM,
            );
            p.region_above(
                t,
                &format!("freecad:tree-toggle:{}", row.name),
                if row.expanded { "Collapse" } else { "Expand" },
            );
        }
        super::icons::draw(p, row.icon, x, y + 3, 16, true);
        let status = model.status.get(&row.name);
        let failed = matches!(status, Some(Status::Error(_)));
        if failed {
            p.circle(x + 13, y + 15, 4, ERROR);
            p.label(x + 9, y + 9, 8, "!", 8, WHITE, true, Align::Center);
        }
        let visible = cad.doc.get(&row.name).is_none_or(|o| o.visible);
        let inactive = matches!(status, Some(Status::Inactive));
        let active_body = cad.active_body.as_deref() == Some(row.name.as_str());
        let color = if failed {
            ERROR
        } else if !visible || inactive {
            Color::rgb(140, 140, 140)
        } else {
            INK
        };
        let label = if tip_of(&row.name) {
            format!("{}  (Tip)", row.label)
        } else {
            row.label.clone()
        };
        match &cad.field {
            Some(Field {
                target: FieldTarget::Label { object },
                text,
                ..
            }) if *object == row.name => {
                let fr = Rect::new(
                    x + 20,
                    y + 1,
                    r.width.saturating_sub((x - r.x) as u32 + 44),
                    ROW_H - 2,
                );
                text_field(p, fr, text, true, "freecad:field-cancel");
            }
            _ => {
                p.label(
                    x + 21,
                    y + 3,
                    r.width.saturating_sub((x - r.x) as u32 + 46),
                    &label,
                    12,
                    color,
                    active_body,
                    Align::Left,
                );
            }
        }
        p.region(rr, &format!("freecad:tree:{}", row.name), &row.label);
        if row.eye {
            let e = Rect::new(r.x + r.width as i32 - 22, y + 3, 16, 16);
            p.symbol(
                if visible { "eye" } else { "eye-off" },
                e.x + 1,
                e.y + 1,
                14,
                if visible {
                    DIM
                } else {
                    Color::rgb(180, 180, 180)
                },
            );
            p.region_above(
                e,
                &format!("freecad:tree-eye:{}", row.name),
                if visible { "Hide" } else { "Show" },
            );
        }
        y += ROW_H as i32;
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(r);
    }
}

fn property_view(cad: &Cad, p: &mut Painter, r: Rect, pointer: Option<(i32, i32)>) {
    p.box_(r, WHITE, 0);
    let col = r.width * 45 / 100;
    let header = Rect::new(r.x, r.y, r.width, 20);
    p.box_(header, Color::rgb(247, 247, 247), 0);
    p.hline(r.x, r.y + 20, r.width, EDGE);
    p.vline(r.x + col as i32, r.y, 20, EDGE);
    p.left(r.x + 6, r.y + 3, col - 8, "Property", 12, INK);
    p.left(
        r.x + col as i32 + 6,
        r.y + 3,
        r.width - col - 8,
        "Value",
        12,
        INK,
    );
    let tabs_h = 22;
    let list = Rect::new(r.x, r.y + 21, r.width, r.height.saturating_sub(21 + tabs_h));
    let mark = p.scene.nodes.len();
    if let Some(object) = cad.selected_object() {
        let mut y = list.y;
        let mut group = "";
        for row in cad.property_rows(object) {
            if y + ROW_H as i32 > list.y + list.height as i32 {
                break;
            }
            if row.group != group {
                group = row.group;
                p.box_(
                    Rect::new(list.x, y, list.width, ROW_H),
                    Color::rgb(228, 228, 228),
                    0,
                );
                p.strong(list.x + 6, y + 3, list.width - 12, group, 12, INK);
                y += ROW_H as i32;
                if y + ROW_H as i32 > list.y + list.height as i32 {
                    break;
                }
            }
            p.hline(list.x, y + ROW_H as i32 - 1, list.width, Color(0, 0, 0, 18));
            p.vline(list.x + col as i32, y, ROW_H, Color(0, 0, 0, 18));
            p.left(list.x + 16, y + 3, col - 20, &row.name, 12, INK);
            let mut vr = Rect::new(list.x + col as i32 + 1, y, list.width - col - 1, ROW_H - 1);
            let editing = matches!(&cad.field, Some(Field { target: FieldTarget::Property { name, .. }, .. }) if *name == row.name);
            let expr_editing = matches!(&cad.field, Some(Field { target: FieldTarget::Expression { name, .. }, .. }) if *name == row.name);
            if row.kind == Kind::Number && !editing {
                // FreeCAD's f(x) button: bind the property to an expression.
                let fx = Rect::new(vr.x + vr.width as i32 - 22, vr.y + 1, 20, ROW_H - 3);
                vr.width = vr.width.saturating_sub(24);
                let bound = row.expression.is_some();
                p.box_(
                    fx,
                    if bound {
                        Color(33, 151, 255, 60)
                    } else {
                        Color::rgb(236, 236, 236)
                    },
                    3,
                );
                p.label(
                    fx.x,
                    fx.y + 2,
                    fx.width,
                    "ƒx",
                    11,
                    if bound { ACCENT } else { DIM },
                    Style::new(false, true, Lang::default()),
                    Align::Center,
                );
                p.region_above(
                    fx,
                    &format!("freecad:prop-expr:{}", row.name),
                    &format!("Expression for {}", row.name),
                );
            }
            match (&row.kind, editing || expr_editing) {
                (_, true) => {
                    let text = cad
                        .field
                        .as_ref()
                        .map(|f| f.text.clone())
                        .unwrap_or_default();
                    let field_r = if expr_editing {
                        Rect::new(vr.x, vr.y, vr.width + 24, vr.height)
                    } else {
                        vr
                    };
                    text_field(p, field_r, &text, true, "freecad:field-cancel");
                }
                (Kind::Bool(on), _) => {
                    checkbox(p, vr.x + 4, vr.y + 4, *on);
                    p.left(vr.x + 22, vr.y + 3, vr.width - 24, &row.value, 12, INK);
                    p.region(
                        vr,
                        &format!("freecad:prop:{}", row.name),
                        &format!("{} = {}", row.name, row.value),
                    );
                }
                (Kind::ReadOnly, _) => {
                    p.left(vr.x + 4, vr.y + 3, vr.width - 6, &row.value, 12, DIM);
                }
                (Kind::Enum(_), _) => {
                    if over(pointer, vr) {
                        p.box_(vr, HOVER, 0);
                    }
                    p.left(vr.x + 4, vr.y + 3, vr.width - 20, &row.value, 12, INK);
                    p.symbol(
                        "chevron-down",
                        vr.x + vr.width as i32 - 14,
                        vr.y + 7,
                        9,
                        DIM,
                    );
                    p.region(
                        vr,
                        &format!("freecad:prop:{}", row.name),
                        &format!("{} = {}", row.name, row.value),
                    );
                }
                _ => {
                    if over(pointer, vr) {
                        p.box_(vr, HOVER, 0);
                    }
                    // A value an expression sets is shown in blue italics, as FreeCAD does.
                    match &row.expression {
                        Some(e) => {
                            p.label(
                                vr.x + 4,
                                vr.y + 3,
                                vr.width - 6,
                                &row.value,
                                12,
                                ACCENT,
                                Style::new(false, true, Lang::default()),
                                Align::Left,
                            );
                            p.region(
                                vr,
                                &format!("freecad:prop:{}", row.name),
                                &format!("{} = {} (bound to {e})", row.name, row.value),
                            );
                        }
                        None => {
                            p.left(vr.x + 4, vr.y + 3, vr.width - 6, &row.value, 12, INK);
                            p.region(
                                vr,
                                &format!("freecad:prop:{}", row.name),
                                &format!("{} = {}", row.name, row.value),
                            );
                        }
                    }
                }
            }
            y += ROW_H as i32;
        }
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(list);
    }
    // View / Data tabs under the table.
    let ty = r.y + r.height as i32 - tabs_h as i32;
    p.hline(r.x, ty, r.width, EDGE);
    let mut x = r.x + 4;
    for (tab, label) in [(PropTab::View, "View"), (PropTab::Data, "Data")] {
        let tw = p.measure(label, 12, false) + 22;
        let t = Rect::new(x, ty, tw, tabs_h - 2);
        let on = cad.props == tab;
        p.border(
            t,
            if on { WHITE } else { Color::rgb(221, 221, 221) },
            2,
            EDGE,
        );
        p.label(
            t.x,
            t.y + 3,
            tw,
            label,
            12,
            if on { INK } else { DIM },
            on,
            Align::Center,
        );
        p.region(
            t,
            &format!("freecad:prop-tab:{}", label.to_lowercase()),
            label,
        );
        x += tw as i32 + 2;
    }
}

// ---------------------------------------------------------------------------------
// Task panels.

fn section(p: &mut Painter, r: Rect, y: i32, title: &str, fold: Option<(&str, bool)>) -> i32 {
    let hr = Rect::new(r.x + 4, y, r.width - 8, 22);
    p.box_(hr, Color::rgb(226, 232, 240), 3);
    p.strong(
        hr.x + 8,
        y + 4,
        hr.width - 30,
        title,
        12,
        Color::rgb(33, 37, 41),
    );
    if let Some((id, folded)) = fold {
        p.symbol(
            if folded { "chevron-down" } else { "chevron-up" },
            hr.x + hr.width as i32 - 16,
            y + 7,
            9,
            DIM,
        );
        p.region(
            hr,
            &format!("freecad:sk:fold:{id}"),
            &format!("{} {title}", if folded { "Expand" } else { "Collapse" }),
        );
    }
    y + 26
}

fn button(
    p: &mut Painter,
    r: Rect,
    label: &str,
    target: &str,
    pointer: Option<(i32, i32)>,
    enabled: Result<(), String>,
) {
    let hover = over(pointer, r) && enabled.is_ok();
    p.border(
        r,
        if hover {
            Color::rgb(253, 253, 253)
        } else {
            Color::rgb(246, 246, 246)
        },
        3,
        EDGE,
    );
    p.label(
        r.x,
        r.y + (r.height as i32 - 17) / 2,
        r.width,
        label,
        12,
        if enabled.is_ok() {
            INK
        } else {
            Color::rgb(150, 150, 150)
        },
        false,
        Align::Center,
    );
    match enabled {
        Ok(()) => p.region(r, target, label),
        Err(why) => {
            p.box_(r, Color::TRANSPARENT, 0);
            p.disabled(&format!("{label}: {why}"));
        }
    }
}

fn checkbox(p: &mut Painter, x: i32, y: i32, on: bool) {
    p.border(Rect::new(x, y, 13, 13), WHITE, 2, Color::rgb(90, 90, 90));
    if on {
        p.line(
            vec![(x + 3, y + 7), (x + 6, y + 10), (x + 11, y + 3)],
            Color::rgb(33, 37, 41),
            2,
        );
    }
}

fn text_field(p: &mut Painter, r: Rect, text: &str, focused: bool, target: &str) {
    p.border(r, WHITE, 2, if focused { ACCENT } else { EDGE });
    let tw = p.left(
        r.x + 4,
        r.y + (r.height as i32 - 17) / 2,
        r.width.saturating_sub(8),
        text,
        12,
        INK,
    );
    if focused {
        p.vline(
            r.x + 5 + tw as i32,
            r.y + 3,
            r.height.saturating_sub(6),
            INK,
        );
    }
    p.region(r, target, "Text field");
}

fn task_panel(cad: &Cad, p: &mut Painter, r: Rect, pointer: Option<(i32, i32)>) {
    p.box_(r, Color::rgb(248, 249, 250), 0);
    let mark = p.scene.nodes.len();
    match &cad.task {
        None => {
            p.center(r.x, r.y + 30, r.width, "No task is open", 12, DIM);
            p.paragraph(
                r.x + 12,
                r.y + 54,
                r.width - 24,
                "Create or double-click a feature to edit its parameters here.",
                12,
                DIM,
            );
        }
        Some(Task::Sketch(e)) => sketch_task(cad, p, r, e, pointer),
        Some(Task::Feature(f)) => feature_task(cad, p, r, f, pointer),
        Some(Task::PickPlane { plane, .. }) => {
            let mut y = r.y + 6;
            ok_cancel(p, r, y, pointer, Ok(()));
            y += 34;
            y = section(p, r, y, "Select feature", None);
            p.paragraph(
                r.x + 12,
                y,
                r.width - 24,
                "Select a plane to attach the sketch to:",
                12,
                INK,
            );
            y += 22;
            let mut choices: Vec<(String, String)> = super::tasks::base_planes()
                .iter()
                .map(|(bp, label)| (bp.name().to_owned(), (*label).to_owned()))
                .collect();
            if let Some(Task::PickPlane { body, .. }) = &cad.task {
                for d in cad.datum_planes(body) {
                    let label = format!("{} (Datum plane)", cad.label_of(&d));
                    choices.push((d, label));
                }
            }
            for (name, label) in choices {
                let row = Rect::new(r.x + 8, y, r.width - 16, ROW_H);
                if name == *plane {
                    p.box_(row, SELECTED, 0);
                }
                super::icons::draw(p, "App::Plane", row.x + 4, row.y + 3, 16, true);
                p.left(row.x + 26, row.y + 3, row.width - 30, &label, 12, INK);
                p.region(row, &format!("freecad:task:plane:{name}"), &label);
                y += ROW_H as i32;
            }
        }
        Some(Task::Measure) => {
            let mut y = r.y + 6;
            let close = Rect::new(r.x + r.width as i32 - 86, y, 78, 26);
            button(p, close, "Close", "freecad:task:ok", pointer, Ok(()));
            let clear = Rect::new(close.x - 124, y, 118, 26);
            button(
                p,
                clear,
                "Clear selection",
                "freecad:task:measure-clear",
                pointer,
                if cad.selection.is_empty() {
                    Err("Nothing is selected".into())
                } else {
                    Ok(())
                },
            );
            y += 34;
            y = section(p, r, y, "Measurement", None);
            let results = cad.measurements();
            if results.is_empty() {
                p.paragraph(r.x + 12, y, r.width - 24, "Select a face, an edge or a vertex in the 3D view; a second pick measures distance and angle. Select a body in the tree for its volume, area and center of mass.", 12, DIM);
            }
            for (k, v) in results {
                p.left(r.x + 12, y, r.width / 2 - 12, &k, 12, DIM);
                p.left(r.x + r.width as i32 / 2, y, r.width / 2 - 8, &v, 12, INK);
                y += 20;
            }
            y += 6;
            for s in &cad.selection {
                let text = if s.sub.is_empty() {
                    cad.label_of(&s.object)
                } else {
                    format!("{}.{}", cad.label_of(&s.object), s.sub)
                };
                p.left(r.x + 12, y, r.width - 24, &text, 11, DIM);
                y += 16;
            }
        }
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(r);
    }
}

fn ok_cancel(
    p: &mut Painter,
    r: Rect,
    y: i32,
    pointer: Option<(i32, i32)>,
    ok: Result<(), String>,
) {
    let cancel = Rect::new(r.x + r.width as i32 - 86, y, 78, 26);
    let okr = Rect::new(cancel.x - 84, y, 78, 26);
    button(p, okr, "OK", "freecad:task:ok", pointer, ok);
    button(p, cancel, "Cancel", "freecad:task:cancel", pointer, Ok(()));
}

fn feature_task(cad: &Cad, p: &mut Painter, r: Rect, f: &FeatureEdit, pointer: Option<(i32, i32)>) {
    let mut y = r.y + 6;
    let model = cad.model();
    ok_cancel(p, r, y, pointer, Ok(()));
    y += 34;
    let kind = cad.doc.get(&f.name).map(|o| o.feature.clone());
    let title = format!(
        "{} parameters",
        kind.as_ref().map(|k| k.base_name()).unwrap_or("Feature")
    );
    y = section(p, r, y, &title, None);
    if let Some(e) = model.error(&f.name) {
        y += p.paragraph(r.x + 12, y, r.width - 24, e, 12, ERROR) as i32 + 6;
    }
    // References: edges of a dress-up, originals of a pattern.
    match &kind {
        Some(Feature::Fillet { edges, .. } | Feature::Chamfer { edges, .. }) => {
            let add = Rect::new(r.x + 12, y, 80, 24);
            let rem = Rect::new(r.x + 98, y, 80, 24);
            for (b, label, mode) in [(add, "Add", true), (rem, "Remove", false)] {
                if f.select_mode == Some(mode) {
                    p.border(b, Color::rgb(216, 216, 216), 3, ACCENT);
                    p.label(b.x, b.y + 4, b.width, label, 12, INK, true, Align::Center);
                    p.region(
                        b,
                        &format!(
                            "freecad:task:select:{}",
                            if mode { "add" } else { "remove" }
                        ),
                        label,
                    );
                } else {
                    button(
                        p,
                        b,
                        label,
                        &format!(
                            "freecad:task:select:{}",
                            if mode { "add" } else { "remove" }
                        ),
                        pointer,
                        Ok(()),
                    );
                }
            }
            y += 30;
            for (i, e) in edges.iter().enumerate() {
                let row = Rect::new(r.x + 12, y, r.width - 24, 20);
                p.border(row, WHITE, 0, Color(0, 0, 0, 20));
                p.left(row.x + 6, row.y + 2, row.width - 40, &e.name, 12, INK);
                let x = Rect::new(row.x + row.width as i32 - 20, row.y + 2, 16, 16);
                if edges.len() > 1 {
                    p.symbol("close", x.x + 2, x.y + 2, 12, DIM);
                    p.region(
                        x,
                        &format!("freecad:task:remove-ref:{i}"),
                        &format!("Remove {}", e.name),
                    );
                }
                y += 20;
            }
            y += 8;
        }
        Some(
            Feature::Mirrored { originals, .. }
            | Feature::LinearPattern { originals, .. }
            | Feature::PolarPattern { originals, .. }
            | Feature::Boolean {
                bodies: originals, ..
            },
        ) => {
            p.paragraph(
                r.x + 12,
                y,
                r.width - 24,
                if matches!(kind, Some(Feature::Boolean { .. })) {
                    "Click bodies in the tree to add or remove them."
                } else {
                    "Click features in the tree to add or remove them."
                },
                11,
                DIM,
            );
            y += 20;
            for (i, o) in originals.iter().enumerate() {
                let row = Rect::new(r.x + 12, y, r.width - 24, 20);
                p.border(row, WHITE, 0, Color(0, 0, 0, 20));
                p.left(
                    row.x + 6,
                    row.y + 2,
                    row.width - 40,
                    &cad.label_of(o),
                    12,
                    INK,
                );
                if originals.len() > 1 {
                    let x = Rect::new(row.x + row.width as i32 - 20, row.y + 2, 16, 16);
                    p.symbol("close", x.x + 2, x.y + 2, 12, DIM);
                    p.region(
                        x,
                        &format!("freecad:task:remove-ref:{i}"),
                        &format!("Remove {o}"),
                    );
                }
                y += 20;
            }
            y += 8;
        }
        _ => {}
    }
    for param in cad.task_params() {
        let (name, label) = match &param {
            Param::Number { name, label, .. }
            | Param::Toggle { name, label, .. }
            | Param::Choice { name, label, .. } => (*name, *label),
        };
        let field = Rect::new(r.x + r.width as i32 / 2, y, r.width / 2 - 12, 22);
        match param {
            Param::Toggle { on, .. } => {
                let row = Rect::new(r.x + 12, y, r.width - 24, 22);
                checkbox(p, row.x, row.y + 4, on);
                p.left(row.x + 20, row.y + 3, row.width - 24, label, 12, INK);
                p.region(row, &format!("freecad:task:toggle:{name}"), label);
            }
            Param::Number { value, .. } => {
                p.left(r.x + 12, y + 3, r.width / 2 - 16, label, 12, INK);
                let focused = matches!(&cad.field, Some(Field { target: FieldTarget::Task { name: n }, .. }) if n == name);
                let text = if focused {
                    cad.field
                        .as_ref()
                        .map(|f| f.text.clone())
                        .unwrap_or_default()
                } else {
                    value
                };
                text_field(
                    p,
                    field,
                    &text,
                    focused,
                    &format!("freecad:field:task:{name}"),
                );
            }
            Param::Choice { value, .. } => {
                p.left(r.x + 12, y + 3, r.width / 2 - 16, label, 12, INK);
                p.border(field, WHITE, 2, EDGE);
                p.left(field.x + 4, field.y + 3, field.width - 20, &value, 12, INK);
                p.symbol(
                    "chevron-down",
                    field.x + field.width as i32 - 14,
                    field.y + 7,
                    9,
                    DIM,
                );
                p.region(field, &format!("freecad:choice:open:task/{name}"), label);
            }
        }
        y += 28;
    }
}

fn sketch_task(cad: &Cad, p: &mut Painter, r: Rect, e: &SketchEdit, pointer: Option<(i32, i32)>) {
    let mut y = r.y + 6;
    let close = Rect::new(r.x + r.width as i32 - 86, y, 78, 26);
    button(p, close, "Close", "freecad:sk:close", pointer, Ok(()));
    y += 34;
    let folded = |s: &str| e.folded.contains(s);
    // Tool parameters for the fillet tool.
    if e.tool == Some(super::sketcher::Tool::Fillet) {
        y = section(p, r, y, "Tool parameters", None);
        p.left(r.x + 12, y + 3, r.width / 2 - 16, "Radius", 12, INK);
        let field = Rect::new(r.x + r.width as i32 / 2, y, r.width / 2 - 12, 22);
        let focused = matches!(&cad.field, Some(Field { target: FieldTarget::Task { name }, .. }) if name == "fillet_radius");
        let text = if focused {
            cad.field
                .as_ref()
                .map(|f| f.text.clone())
                .unwrap_or_default()
        } else {
            format!("{} mm", fmt_num(e.fillet_radius, 2))
        };
        text_field(p, field, &text, focused, "freecad:sk:fillet-radius");
        y += 30;
    }
    y = section(
        p,
        r,
        y,
        "Solver messages",
        Some(("solver", folded("solver"))),
    );
    if !folded("solver") {
        let msg = e.report.message();
        let color = match e.report.status {
            SolveStatus::FullyConstrained => OK_GREEN,
            SolveStatus::UnderConstrained | SolveStatus::Empty => INK,
            SolveStatus::Redundant => WARN,
            _ => ERROR,
        };
        let tw = p.left(r.x + 12, y, r.width - 24, &msg, 12, color);
        if e.report.status == SolveStatus::UnderConstrained {
            // FreeCAD underlines the DoF count; clicking it selects the free geometry.
            p.hline(r.x + 12, y + 16, tw, color);
            p.region(
                Rect::new(r.x + 12, y, tw, 18),
                "freecad:sk:select-free",
                "Select the geometry that is not yet constrained",
            );
        }
        if matches!(
            e.report.status,
            SolveStatus::Conflicting | SolveStatus::Redundant
        ) && cad
            .available("Sketcher_SelectConflictingConstraints")
            .is_ok()
        {
            p.region(
                Rect::new(r.x + 12, y, tw, 18),
                "freecad:cmd:Sketcher_SelectConflictingConstraints",
                "Select the listed constraints",
            );
        }
        y += 24;
    }
    y = section(p, r, y, "Edit controls", Some(("edit", folded("edit"))));
    if !folded("edit") {
        let row = Rect::new(r.x + 12, y, r.width - 24, 22);
        checkbox(p, row.x, row.y + 4, e.construction);
        p.left(
            row.x + 20,
            row.y + 3,
            row.width - 24,
            "Construction geometry",
            12,
            INK,
        );
        p.region(row, "freecad:sk:construction", "Construction geometry");
        y += 28;
    }
    let s = cad.sketch();
    y = section(
        p,
        r,
        y,
        "Constraints",
        Some(("constraints", folded("constraints"))),
    );
    if !folded("constraints") {
        if let Some(s) = s {
            let conflict: BTreeSet<usize> =
                e.report.conflict_groups.iter().map(|i| i - 1).collect();
            for (i, c) in s.constraints.iter().enumerate() {
                if y + 20 > r.y + r.height as i32 - 60 {
                    p.left(
                        r.x + 12,
                        y,
                        r.width - 24,
                        &format!("… {} more", s.constraints.len() - i),
                        11,
                        DIM,
                    );
                    y += 20;
                    break;
                }
                let row = Rect::new(r.x + 8, y, r.width - 16, 20);
                if e.constraints.contains(&i) {
                    p.box_(row, SELECTED, 0);
                }
                let name = if c.name.is_empty() {
                    format!("Constraint{}", i + 1)
                } else {
                    c.name.clone()
                };
                let text = if c.kind.is_dimensional() {
                    format!("{name} ({})", c.display_value())
                } else {
                    name
                };
                let color = if conflict.contains(&i) {
                    ERROR
                } else if !c.driving {
                    DIM
                } else {
                    INK
                };
                super::icons::constraint_badge(p, c.kind, row.x + 2, row.y + 2);
                p.left(row.x + 22, row.y + 2, row.width - 26, &text, 12, color);
                p.region(row, &format!("freecad:sk:constraint:{i}"), &text);
                y += 20;
            }
        }
        y += 6;
    }
    y = section(p, r, y, "Elements", Some(("elements", folded("elements"))));
    if !folded("elements") {
        if let Some(s) = s {
            for (i, g) in s.geos.iter().enumerate() {
                if y + 20 > r.y + r.height as i32 {
                    break;
                }
                let row = Rect::new(r.x + 8, y, r.width - 16, 20);
                if e.picked.contains(&(i as i32, Pos::None)) {
                    p.box_(row, SELECTED, 0);
                }
                let text = format!(
                    "{}-{}{}",
                    i + 1,
                    g.geom.type_name(),
                    if g.construction {
                        " (construction)"
                    } else {
                        ""
                    }
                );
                p.left(
                    row.x + 6,
                    row.y + 2,
                    row.width - 10,
                    &text,
                    12,
                    if g.construction {
                        Color::rgb(59, 91, 219)
                    } else {
                        INK
                    },
                );
                p.region(row, &format!("freecad:sk:element:{i}"), &text);
                y += 20;
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Report view and status bar.

fn report_view(cad: &Cad, p: &mut Painter, r: Rect) {
    p.box_(r, BG, 0);
    p.hline(r.x, r.y, r.width, EDGE);
    p.strong(r.x + 6, r.y + 3, 200, "Report view", 12, INK);
    let close = Rect::new(r.x + r.width as i32 - 22, r.y + 2, 18, 18);
    p.symbol("close", close.x + 3, close.y + 3, 12, DIM);
    p.region(close, "freecad:report-close", "Close report view");
    let clear = Rect::new(close.x - 50, r.y + 2, 44, 18);
    if !cad.report.is_empty() {
        p.label(
            clear.x,
            clear.y + 1,
            clear.width,
            "Clear",
            11,
            DIM,
            false,
            Align::Center,
        );
        p.region(clear, "freecad:report-clear", "Clear the report view");
    }
    let body = Rect::new(r.x + 2, r.y + 22, r.width - 4, r.height.saturating_sub(24));
    p.box_(body, WHITE, 0);
    let lines = (body.height / 16).max(1) as usize;
    let start = cad.report.len().saturating_sub(lines);
    let mut y = body.y + 1;
    for (kind, text) in &cad.report[start..] {
        let color = match kind {
            ReportKind::Log => INK,
            ReportKind::Warning => WARN,
            ReportKind::Error => ERROR,
        };
        p.left(body.x + 4, y, body.width - 8, text, 11, color);
        y += 16;
    }
}

fn status_bar(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let r = l.status;
    p.box_(r, BG, 0);
    p.hline(r.x, r.y, r.width, Color(0, 0, 0, 25));
    let pre = super::view3d::preselection(cad, pointer, l);
    let text = super::view3d::preselection_text(cad, &pre)
        .or_else(|| {
            cad.sketch_edit()
                .and_then(|_| super::view3d::sketch_hover_text(cad, pointer, l))
        })
        .unwrap_or_else(|| cad.status.clone());
    let right_w = 330;
    p.left(
        r.x + 6,
        r.y + 4,
        r.width.saturating_sub(right_w + 12),
        &text,
        12,
        INK,
    );
    // The view's extent at the target, as FreeCAD shows it.
    let (vw, vh) = (l.view.width.max(1), l.view.height.max(1));
    let hh = cad.camera.half_height;
    let dims = format!(
        "{} mm x {} mm",
        fmt_num(2.0 * hh * f64::from(vw) / f64::from(vh), 2),
        fmt_num(2.0 * hh, 2)
    );
    let dw = p.measure(&dims, 12, false) + 12;
    p.right(
        r.x + r.width as i32 - dw as i32 - 6,
        r.y + 4,
        dw,
        &dims,
        12,
        DIM,
    );
    let nav = Rect::new(
        r.x + r.width as i32 - dw as i32 - 130,
        r.y + 2,
        118,
        r.height - 4,
    );
    if over(pointer, nav) || cad.menu.as_deref() == Some("nav") {
        p.border(nav, Color::TRANSPARENT, 3, EDGE);
    }
    super::icons::draw(p, "Mouse", nav.x + 4, nav.y + 2, 16, true);
    p.left(
        nav.x + 24,
        nav.y + 2,
        nav.width - 26,
        cad.nav.label(),
        12,
        INK,
    );
    p.region(
        nav,
        "freecad:nav-menu",
        &format!("Navigation style: {}. {}", cad.nav.label(), cad.nav.hint()),
    );
}

// ---------------------------------------------------------------------------------
// Pop-up menus.

/// A popup menu row: (label, target, enabled, shortcut); an empty label is a separator.
type PopupItem = (String, Option<String>, Result<(), String>, String);

fn popup(
    p: &mut Painter,
    l: &Layout,
    x: i32,
    y: i32,
    items: &[PopupItem],
    pointer: Option<(i32, i32)>,
) {
    let width = items
        .iter()
        .map(|(label, _, _, keys)| p.measure(label, 12, false) + p.measure(keys, 12, false) + 64)
        .max()
        .unwrap_or(120)
        .clamp(140, 360);
    let height: u32 = items
        .iter()
        .map(|i| if i.0.is_empty() { 7 } else { 22 })
        .sum::<u32>()
        + 6;
    let x = x.min(l.w as i32 - width as i32 - 2).max(0);
    let y = y.min(l.h as i32 - height as i32 - 2).max(0);
    p.drop_shadow(Rect::new(x, y, width, height), 2, 6, 60, 2);
    p.border(Rect::new(x, y, width, height), WHITE, 2, EDGE);
    let mut cy = y + 3;
    for (label, target, enabled, keys) in items {
        if label.is_empty() {
            p.hline(x + 6, cy + 3, width - 12, Color(0, 0, 0, 30));
            cy += 7;
            continue;
        }
        let row = Rect::new(x + 2, cy, width - 4, 22);
        let live = enabled.is_ok() && target.is_some();
        if live && over(pointer, row) {
            p.box_(row, SELECTED, 0);
        }
        p.left(
            row.x + 26,
            row.y + 3,
            width - if keys.is_empty() { 36 } else { 100 },
            label,
            12,
            if live { INK } else { Color::rgb(150, 150, 150) },
        );
        if !keys.is_empty() {
            p.right(row.x + width as i32 - 110, row.y + 3, 100, keys, 12, DIM);
        }
        match (target, enabled) {
            (Some(t), Ok(())) => p.region_above(row, t, label),
            (_, Err(why)) => {
                p.box_(row, Color::TRANSPARENT, 0);
                p.disabled(&format!("{label}: {why}"));
            }
            _ => {}
        }
        cy += 22;
    }
}

fn popups(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let Some(menu) = cad.menu.clone() else { return };
    p.z += 50;
    // A click anywhere outside the pop-up closes it.
    p.region_above(
        Rect::new(0, 0, l.w, l.h),
        "freecad:menu-close",
        "Close menu",
    );
    p.z += 5;
    let cmd_item = |id: &str| -> (String, Option<String>, Result<(), String>, String) {
        let c = command(id);
        (
            c.map(|c| c.label).unwrap_or(id).to_owned(),
            Some(format!("freecad:cmd:{id}")),
            cad.available(id),
            c.map(|c| c.keys).unwrap_or("").to_owned(),
        )
    };
    let sep = || (String::new(), None, Ok(()), String::new());
    if let Some((name, items)) = MENUS.iter().find(|(n, _)| *n == menu) {
        let mut x = 4;
        for (n, _) in MENUS {
            if n == name {
                break;
            }
            x += p.measure(n, 12, false) as i32 + 16;
        }
        let list: Vec<_> = items
            .iter()
            .map(|id| if *id == "-" { sep() } else { cmd_item(id) })
            .collect();
        let y = l.menu.map(|m| m.y + m.height as i32).unwrap_or(0);
        popup(p, l, x, y, &list, pointer);
    } else if menu == "workbench" {
        let (placed, _) = toolbar_items(cad, p, l);
        let at = placed
            .iter()
            .find(|(id, _)| *id == "workbench")
            .map(|(_, r)| *r)
            .unwrap_or(l.toolbar);
        let list = vec![
            (
                "Part Design".to_owned(),
                Some("freecad:wb:Part Design".to_owned()),
                Ok(()),
                String::new(),
            ),
            (
                "Sketcher".to_owned(),
                Some("freecad:wb:Sketcher".to_owned()),
                if cad.task.is_some() && cad.sketch_edit().is_none() {
                    Err("Close the open task first".to_owned())
                } else {
                    Ok(())
                },
                String::new(),
            ),
        ];
        popup(p, l, at.x, at.y + at.height as i32, &list, pointer);
    } else if menu == "overflow" {
        let (_, overflow) = toolbar_items(cad, p, l);
        let list: Vec<_> = overflow.iter().map(|id| cmd_item(id)).collect();
        popup(
            p,
            l,
            l.w as i32 - 240,
            l.toolbar.y + l.toolbar.height as i32,
            &list,
            pointer,
        );
    } else if menu == "nav" {
        let list: Vec<_> = NavStyle::ALL
            .iter()
            .map(|n| {
                let mark = if *n == cad.nav { "✓ " } else { "" };
                (
                    format!("{mark}{}", n.label()),
                    Some(format!("freecad:nav:{}", n.label())),
                    Ok(()),
                    String::new(),
                )
            })
            .collect();
        popup(p, l, l.w as i32 - 300, l.status.y - 60, &list, pointer);
    } else if menu == "navcube" {
        let list: Vec<_> = [
            "Std_OrthographicCamera",
            "Std_PerspectiveCamera",
            "Std_ViewIsometric",
            "Std_ViewFitAll",
        ]
        .iter()
        .map(|id| {
            let (label, _, ok, keys) = cmd_item(id);
            (label, Some(format!("freecad:navcube-view:{id}")), ok, keys)
        })
        .collect();
        let c = l.cube();
        popup(
            p,
            l,
            l.view.x + c.x + c.width as i32 - 180,
            l.view.y + c.y + c.height as i32,
            &list,
            pointer,
        );
    } else if let Some(target) = menu.strip_prefix("dd:") {
        let options: Vec<(String, String)> = if let Some(name) = target.strip_prefix("task/") {
            cad.task_params()
                .into_iter()
                .find_map(|pm| match pm {
                    Param::Choice {
                        name: n, options, ..
                    } if n == name => Some(options),
                    _ => None,
                })
                .unwrap_or_default()
        } else if let Some(name) = target.strip_prefix("prop/") {
            cad.selected_object()
                .and_then(|o| cad.property_rows(o).into_iter().find(|r| r.name == name))
                .and_then(|r| match r.kind {
                    Kind::Enum(v) => Some(v.into_iter().map(|x| (x.clone(), x)).collect()),
                    _ => None,
                })
                .unwrap_or_default()
        } else if target == "filetype" {
            match &cad.dialog {
                Some(Dialog::File(d)) => super::files::filters(d.purpose)
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (i.to_string(), f.0.to_owned()))
                    .collect(),
                _ => vec![],
            }
        } else if target == "filepath" {
            // The Mac panel's folder pop-up: this folder, then each one above it.
            match &cad.dialog {
                Some(Dialog::File(d)) => {
                    let mut out = vec![];
                    let mut path = d.folder.trim_end_matches('/').to_owned();
                    while !path.is_empty() {
                        let name = path.rsplit('/').next().unwrap_or("").to_owned();
                        out.push((path.clone(), name));
                        path = match path.rsplit_once('/') {
                            Some((up, _)) => up.to_owned(),
                            None => String::new(),
                        };
                    }
                    out.push(("/".to_owned(), "Macintosh HD".to_owned()));
                    out
                }
                _ => vec![],
            }
        } else {
            vec![]
        };
        let list: Vec<_> = options
            .into_iter()
            .map(|(value, label)| {
                (
                    label,
                    Some(format!("freecad:choice:{target}:{value}")),
                    Ok(()),
                    String::new(),
                )
            })
            .collect();
        // The list drops down under the control that opened it.
        let opener = match target.strip_prefix("prop/") {
            Some(name) => format!("freecad:prop:{name}"),
            None => format!("freecad:choice:open:{target}"),
        };
        let anchor = p
            .scene
            .nodes
            .iter()
            .rev()
            .find(|n| n.interaction.as_deref() == Some(opener.as_str()))
            .map(|n| n.bounds)
            .unwrap_or(Rect::new(l.w as i32 / 2, l.h as i32 / 2, 0, 0));
        popup(
            p,
            l,
            anchor.x,
            anchor.y + anchor.height as i32,
            &list,
            pointer,
        );
    }
}

// ---------------------------------------------------------------------------------
// Dialogs.

fn dialog_frame(p: &mut Painter, l: &Layout, w: u32, h: u32, title: &str) -> Rect {
    p.z += 200;
    p.box_(Rect::new(0, 0, l.w, l.h), Color(0, 0, 0, 40), 0);
    // A modal dialog holds the pointer: clicks outside it only remind.
    p.region(
        Rect::new(0, 0, l.w, l.h),
        "freecad:dialog:block",
        "Finish the dialog first",
    );
    let w = w.min(l.w.saturating_sub(20));
    let h = h.min(l.h.saturating_sub(20));
    let r = Rect::new(
        (l.w as i32 - w as i32) / 2,
        (l.h as i32 - h as i32) / 2,
        w,
        h,
    );
    p.drop_shadow(r, 4, 12, 70, 4);
    p.border(r, BG, 4, EDGE);
    p.box_(
        Rect::new(r.x + 1, r.y + 1, r.width - 2, 26),
        Color::rgb(230, 230, 230),
        3,
    );
    p.strong(r.x + 10, r.y + 5, r.width - 20, title, 12, INK);
    Rect::new(r.x + 10, r.y + 34, r.width - 20, r.height - 44)
}

fn dialogs(cad: &Cad, p: &mut Painter, l: &Layout, env: &crate::AppEnv<'_>) {
    let pointer = env.pointer;
    let Some(d) = &cad.dialog else { return };
    match d {
        Dialog::File(fd) => super::file_dialog::draw(cad, p, l, fd, env),
        Dialog::Dimension { index, title } => {
            let r = dialog_frame(p, l, 300, 130, title);
            let label = match cad
                .sketch()
                .and_then(|s| s.constraints.get(*index))
                .map(|c| c.kind)
            {
                Some(T::Angle) => "Angle:",
                Some(T::Radius) => "Radius:",
                Some(T::Diameter) => "Diameter:",
                _ => "Length:",
            };
            p.left(r.x, r.y + 6, 80, label, 12, INK);
            let text = cad
                .field
                .as_ref()
                .map(|f| f.text.clone())
                .unwrap_or_default();
            text_field(
                p,
                Rect::new(r.x + 80, r.y + 2, r.width - 80, 24),
                &text,
                cad.field.is_some(),
                &format!("freecad:field:constraint:{index}"),
            );
            let cancel = Rect::new(
                r.x + r.width as i32 - 80,
                r.y + r.height as i32 - 28,
                80,
                26,
            );
            let ok = Rect::new(cancel.x - 86, cancel.y, 80, 26);
            button(p, ok, "OK", "freecad:dialog:ok", pointer, Ok(()));
            button(
                p,
                cancel,
                "Cancel",
                "freecad:dialog:cancel",
                pointer,
                Ok(()),
            );
        }
        Dialog::Unsaved { .. } => {
            let r = dialog_frame(p, l, 420, 150, "Unsaved document");
            p.paragraph(
                r.x,
                r.y,
                r.width,
                &format!(
                    "The document '{}' has been modified. Do you want to save your changes?",
                    cad.doc.label
                ),
                12,
                INK,
            );
            let cancel = Rect::new(
                r.x + r.width as i32 - 80,
                r.y + r.height as i32 - 28,
                80,
                26,
            );
            let discard = Rect::new(cancel.x - 86, cancel.y, 80, 26);
            let save = Rect::new(discard.x - 86, cancel.y, 80, 26);
            button(p, save, "Save", "freecad:dialog:save", pointer, Ok(()));
            button(
                p,
                discard,
                "Discard",
                "freecad:dialog:discard",
                pointer,
                Ok(()),
            );
            button(
                p,
                cancel,
                "Cancel",
                "freecad:dialog:cancel",
                pointer,
                Ok(()),
            );
        }
        Dialog::Message { title, text } => {
            let r = dialog_frame(p, l, 420, 150, title);
            p.paragraph(r.x, r.y, r.width, text, 12, INK);
            let ok = Rect::new(
                r.x + r.width as i32 - 80,
                r.y + r.height as i32 - 28,
                80,
                26,
            );
            button(p, ok, "OK", "freecad:dialog:ok", pointer, Ok(()));
        }
        Dialog::About => {
            let r = dialog_frame(p, l, 420, 190, "About FreeCAD");
            super::icons::draw(p, "FreeCAD", r.x, r.y, 48, true);
            p.strong(r.x + 60, r.y + 4, r.width - 60, "FreeCAD 1.0.2", 16, INK);
            p.paragraph(
                r.x + 60,
                r.y + 30,
                r.width - 60,
                "Your own 3D parametric modeler. Part Design and Sketcher over the simulator's deterministic geometry kernel.",
                12,
                DIM,
            );
            let ok = Rect::new(
                r.x + r.width as i32 - 80,
                r.y + r.height as i32 - 28,
                80,
                26,
            );
            button(p, ok, "OK", "freecad:dialog:ok", pointer, Ok(()));
        }
    }
}
