//! The realm state outside the VM heap, as a heap snapshot carries it: everything
//! script can observe or that decides what it will observe next (the document,
//! stylesheets as their sources, interaction and form state, canvases, the task
//! queues of the document loader, history, transitions in flight), but not what a
//! style or layout flush recomputes (computed styles, the fragment tree, hit
//! lists, layout caches), which a restored realm rebuilds on first use. The JS
//! values the realm holds (node wrappers, the prelude's prototypes) are written as
//! heap roots.

use std::collections::{BTreeMap, BTreeSet};

use cw_jsvm::value::{Obj, Value};
use serde::{Deserialize, Serialize};

use super::{AnimState, AnimationStart, HistoryEntry, Inner, LogEntry, LogLevel, SheetEntry};
use super::{FormData, SheetOwner};
use crate::css::{self, Origin};
use crate::dom::{Document, NodeId};
use crate::geom::Au;
use crate::script::canvas::CanvasState;
use crate::{Strictness, Viewport};

#[derive(Serialize, Deserialize)]
struct SheetImage {
    id: u32,
    /// 0 an element (`node`), 1 constructed, 2 imported by sheet `node`.
    owner: (u8, u32),
    disabled: bool,
    media: String,
    href: Option<String>,
    /// The text the sheet came from (what a `<style>` change is detected against).
    source: String,
    /// The sheet's rules as text, when script edited them (see `sheet_image`).
    text: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AnimImage {
    node: u32,
    values: Vec<(String, String)>,
    names: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct AnimationStartImage {
    node: u32,
    name: String,
    is_animation: bool,
    delay_ms: i32,
    duration_ms: i32,
    iterations: Option<f64>,
    cancelled: bool,
}

/// See the module documentation.
#[derive(Serialize, Deserialize)]
pub(crate) struct InnerImage {
    journal_recording: bool,
    doc: Document,
    url: String,
    viewport: (u32, u32, u8, u16),
    sheets: Vec<SheetImage>,
    constructed: Vec<SheetImage>,
    adopted: Vec<u32>,
    next_sheet_id: u32,
    sheets_dirty: bool,
    generation: u64,
    images: BTreeMap<String, (u32, u32)>,
    scroll: Vec<(u32, i32, i32)>,
    hovered: Option<u32>,
    pointer: Option<(i32, i32)>,
    hover_generation: u64,
    inline_handlers: (u64, u64, bool),
    active: Option<u32>,
    focused: Option<u32>,
    focus_visible: bool,
    target_id: Option<String>,
    form_values: BTreeMap<u32, String>,
    form_checked: BTreeMap<u32, bool>,
    form_indeterminate: BTreeSet<u32>,
    form_selection: BTreeMap<u32, (u64, u64)>,
    form_validity: BTreeMap<u32, String>,
    /// How many of the heap roots are `wrappers` (the rest are `protos`).
    wrappers: u64,
    protos: Vec<String>,
    logs: Vec<(u8, String)>,
    canvases: Vec<(u32, CanvasState)>,
    module_sources: BTreeMap<String, String>,
    parsing: bool,
    write_buffer: String,
    ready_state: String,
    current_script: Option<u32>,
    deferred_scripts: Vec<u32>,
    async_scripts: Vec<u32>,
    pending_scripts: Vec<u32>,
    executed_scripts: Vec<u32>,
    has_raf: bool,
    has_layout_observers: bool,
    observed_generation: u64,
    observing: bool,
    custom_defined: BTreeSet<String>,
    history: Vec<(String, Option<String>)>,
    history_index: u64,
    hidden: bool,
    alerts: Vec<String>,
    referrer: String,
    cookie_cache: Option<String>,
    dom_mutations_since_styles: u64,
    anim_state: Vec<AnimImage>,
    pending_animations: Vec<AnimationStartImage>,
}

fn level(l: LogLevel) -> u8 {
    match l {
        LogLevel::Log => 0,
        LogLevel::Info => 1,
        LogLevel::Warn => 2,
        LogLevel::Error => 3,
        LogLevel::Debug => 4,
    }
}

fn level_of(b: u8) -> LogLevel {
    match b {
        1 => LogLevel::Info,
        2 => LogLevel::Warn,
        3 => LogLevel::Error,
        4 => LogLevel::Debug,
        _ => LogLevel::Log,
    }
}

fn ids(v: &[NodeId]) -> Vec<u32> {
    v.iter().map(|n| n.0).collect()
}

fn nodes(v: Vec<u32>) -> Vec<NodeId> {
    v.into_iter().map(NodeId).collect()
}

/// A sheet as its source, and as the text of its rules when script edited it
/// through the CSSOM (so the source no longer parses to it). Refused when neither
/// parses back to exactly the sheet.
fn sheet_image(e: &SheetEntry) -> Result<SheetImage, String> {
    let parsed =
        css::parse_stylesheet(&e.source, Origin::Author, Strictness::Lenient).unwrap_or_default();
    let text = if parsed == e.sheet {
        None
    } else {
        let text: Vec<String> = e
            .sheet
            .rules
            .iter()
            .map(crate::script::bindings::style::rule_css_text)
            .collect();
        let text = text.join("\n");
        let mut again =
            css::parse_stylesheet(&text, Origin::Author, Strictness::Lenient).unwrap_or_default();
        again.unsupported = parsed.unsupported;
        if again != e.sheet {
            return Err(format!(
                "stylesheet {} was edited through the CSSOM into a form its text does not \
                 parse back to",
                e.id
            ));
        }
        Some(text)
    };
    Ok(SheetImage {
        id: e.id,
        owner: match e.owner {
            SheetOwner::Element(n) => (0, n.0),
            SheetOwner::Constructed => (1, 0),
            SheetOwner::Import(i) => (2, i),
        },
        disabled: e.disabled,
        media: e.media.clone(),
        href: e.href.clone(),
        source: e.source.clone(),
        text,
    })
}

fn sheet_of(s: SheetImage) -> SheetEntry {
    let parse =
        |t: &str| css::parse_stylesheet(t, Origin::Author, Strictness::Lenient).unwrap_or_default();
    let sheet = match &s.text {
        None => parse(&s.source),
        Some(text) => {
            let mut sheet = parse(text);
            sheet.unsupported = parse(&s.source).unsupported;
            sheet
        }
    };
    SheetEntry {
        id: s.id,
        owner: match s.owner {
            (0, n) => SheetOwner::Element(NodeId(n)),
            (1, _) => SheetOwner::Constructed,
            (_, i) => SheetOwner::Import(i),
        },
        sheet,
        disabled: s.disabled,
        media: s.media,
        href: s.href,
        source: s.source,
    }
}

impl Inner {
    /// This state's image, and the JS values it holds (the heap roots it names).
    pub(crate) fn image(&self) -> Result<(InnerImage, Vec<Value>), String> {
        let sheets = self
            .sheets
            .iter()
            .map(sheet_image)
            .collect::<Result<_, _>>()?;
        let constructed = self
            .constructed
            .values()
            .map(sheet_image)
            .collect::<Result<_, _>>()?;
        let mut roots: Vec<Value> = self
            .wrappers
            .iter()
            .map(|w| w.clone().map_or(Value::Undefined, Value::Obj))
            .collect();
        roots.extend(self.protos.values().map(|o| Value::Obj(o.clone())));
        let f = &self.form;
        let img = InnerImage {
            journal_recording: self.journal.recording,
            doc: self.doc.clone(),
            url: self.url.clone(),
            viewport: (
                self.viewport.width,
                self.viewport.height,
                self.viewport.scale,
                self.viewport.zoom,
            ),
            sheets,
            constructed,
            adopted: self.adopted.clone(),
            next_sheet_id: self.next_sheet_id,
            sheets_dirty: self.sheets_dirty,
            generation: self.generation,
            images: self.images.0.clone(),
            scroll: self
                .scroll
                .iter()
                .map(|(n, (x, y))| (n.0, x.0, y.0))
                .collect(),
            hovered: self.hovered.map(|n| n.0),
            pointer: self.pointer,
            hover_generation: self.hover_generation,
            inline_handlers: (
                self.inline_handlers.0,
                self.inline_handlers.1 as u64,
                self.inline_handlers.2,
            ),
            active: self.active.map(|n| n.0),
            focused: self.focused.map(|n| n.0),
            focus_visible: self.focus_visible,
            target_id: self.target_id.clone(),
            form_values: f.values.iter().map(|(n, v)| (n.0, v.clone())).collect(),
            form_checked: f.checked.iter().map(|(n, v)| (n.0, *v)).collect(),
            form_indeterminate: f.indeterminate.iter().map(|n| n.0).collect(),
            form_selection: f
                .selection
                .iter()
                .map(|(n, (a, b))| (n.0, (*a as u64, *b as u64)))
                .collect(),
            form_validity: f
                .custom_validity
                .iter()
                .map(|(n, v)| (n.0, v.clone()))
                .collect(),
            wrappers: self.wrappers.len() as u64,
            protos: self.protos.keys().cloned().collect(),
            logs: self
                .logs
                .iter()
                .map(|l| (level(l.level), l.text.clone()))
                .collect(),
            canvases: self
                .canvases
                .iter()
                .map(|(n, c)| (n.0, c.clone()))
                .collect(),
            module_sources: self.module_sources.clone(),
            parsing: self.parsing,
            write_buffer: self.write_buffer.clone(),
            ready_state: self.ready_state.clone(),
            current_script: self.current_script.map(|n| n.0),
            deferred_scripts: ids(&self.deferred_scripts),
            async_scripts: ids(&self.async_scripts),
            pending_scripts: ids(&self.pending_scripts),
            executed_scripts: self.executed_scripts.iter().map(|n| n.0).collect(),
            has_raf: self.has_raf,
            has_layout_observers: self.has_layout_observers,
            observed_generation: self.observed_generation,
            observing: self.observing,
            custom_defined: self.custom_defined.clone(),
            history: self
                .history
                .iter()
                .map(|h| (h.url.clone(), h.state.clone()))
                .collect(),
            history_index: self.history_index as u64,
            hidden: self.hidden,
            alerts: self.alerts.clone(),
            referrer: self.referrer.clone(),
            cookie_cache: self.cookie_cache.clone(),
            dom_mutations_since_styles: self.dom_mutations_since_styles as u64,
            anim_state: self
                .anim_state
                .iter()
                .map(|(n, a)| AnimImage {
                    node: n.0,
                    values: a.values.clone(),
                    names: a.names.clone(),
                })
                .collect(),
            pending_animations: self
                .pending_animations
                .iter()
                .map(|a| AnimationStartImage {
                    node: a.node.0,
                    name: a.name.clone(),
                    is_animation: a.is_animation,
                    delay_ms: a.delay_ms,
                    duration_ms: a.duration_ms,
                    iterations: a.iterations,
                    cancelled: a.cancelled,
                })
                .collect(),
        };
        Ok((img, roots))
    }

    /// Puts an image's state into this (freshly made) state; `roots` are the heap
    /// roots `image` returned, as the restored VM gave them back. Styles, layout
    /// and hit lists are left to be computed on first use.
    pub(crate) fn apply_image(&mut self, img: InnerImage, roots: &[Value]) -> Result<(), String> {
        let nw = img.wrappers as usize;
        if roots.len() != nw + img.protos.len() {
            return Err("heap roots do not match the realm image".into());
        }
        let obj = |v: &Value| -> Option<Obj> {
            match v {
                Value::Obj(o) => Some(o.clone()),
                _ => None,
            }
        };
        self.journal.recording = img.journal_recording;
        self.doc = img.doc;
        self.url = img.url;
        self.viewport = Viewport {
            width: img.viewport.0,
            height: img.viewport.1,
            scale: img.viewport.2,
            zoom: img.viewport.3,
        };
        self.sheets = img.sheets.into_iter().map(sheet_of).collect();
        self.constructed = img
            .constructed
            .into_iter()
            .map(|s| (s.id, sheet_of(s)))
            .collect();
        self.adopted = img.adopted;
        self.next_sheet_id = img.next_sheet_id;
        self.sheets_dirty = img.sheets_dirty;
        self.generation = img.generation;
        self.images.0 = img.images;
        self.scroll = img
            .scroll
            .into_iter()
            .map(|(n, x, y)| (NodeId(n), (Au(x), Au(y))))
            .collect();
        self.hovered = img.hovered.map(NodeId);
        self.pointer = img.pointer;
        self.hover_generation = img.hover_generation;
        self.inline_handlers = (
            img.inline_handlers.0,
            img.inline_handlers.1 as usize,
            img.inline_handlers.2,
        );
        self.active = img.active.map(NodeId);
        self.focused = img.focused.map(NodeId);
        self.focus_visible = img.focus_visible;
        self.target_id = img.target_id;
        self.form = FormData {
            values: img
                .form_values
                .into_iter()
                .map(|(n, v)| (NodeId(n), v))
                .collect(),
            checked: img
                .form_checked
                .into_iter()
                .map(|(n, v)| (NodeId(n), v))
                .collect(),
            indeterminate: img.form_indeterminate.into_iter().map(NodeId).collect(),
            selection: img
                .form_selection
                .into_iter()
                .map(|(n, (a, b))| (NodeId(n), (a as usize, b as usize)))
                .collect(),
            custom_validity: img
                .form_validity
                .into_iter()
                .map(|(n, v)| (NodeId(n), v))
                .collect(),
        };
        self.wrappers = roots[..nw].iter().map(obj).collect();
        self.protos = img
            .protos
            .into_iter()
            .zip(&roots[nw..])
            .filter_map(|(k, v)| obj(v).map(|o| (k, o)))
            .collect();
        self.logs = img
            .logs
            .into_iter()
            .map(|(l, text)| LogEntry {
                level: level_of(l),
                text,
            })
            .collect();
        self.canvases = img
            .canvases
            .into_iter()
            .map(|(n, c)| (NodeId(n), c))
            .collect();
        self.module_sources = img.module_sources;
        self.parsing = img.parsing;
        self.write_buffer = img.write_buffer;
        self.ready_state = img.ready_state;
        self.current_script = img.current_script.map(NodeId);
        self.deferred_scripts = nodes(img.deferred_scripts);
        self.async_scripts = nodes(img.async_scripts);
        self.pending_scripts = nodes(img.pending_scripts);
        self.executed_scripts = img.executed_scripts.into_iter().map(NodeId).collect();
        self.has_raf = img.has_raf;
        self.has_layout_observers = img.has_layout_observers;
        self.observed_generation = img.observed_generation;
        self.observing = img.observing;
        self.custom_defined = img.custom_defined;
        self.history = img
            .history
            .into_iter()
            .map(|(url, state)| HistoryEntry { url, state })
            .collect();
        self.history_index = img.history_index as usize;
        self.hidden = img.hidden;
        self.alerts = img.alerts;
        self.referrer = img.referrer;
        self.cookie_cache = img.cookie_cache;
        self.dom_mutations_since_styles = img.dom_mutations_since_styles as usize;
        // The style a transition is measured against is compared by address only
        // (an element whose style did not change skips the comparison); a fresh
        // one matches nothing, so the next flush compares the values themselves.
        let placeholder = std::rc::Rc::new(crate::style::ComputedStyle::initial());
        self.anim_state = img
            .anim_state
            .into_iter()
            .map(|a| {
                (
                    NodeId(a.node),
                    AnimState {
                        style: placeholder.clone(),
                        values: a.values,
                        names: a.names,
                    },
                )
            })
            .collect();
        self.pending_animations = img
            .pending_animations
            .into_iter()
            .map(|a| AnimationStart {
                node: NodeId(a.node),
                name: a.name,
                is_animation: a.is_animation,
                delay_ms: a.delay_ms,
                duration_ms: a.duration_ms,
                iterations: a.iterations,
                cancelled: a.cancelled,
            })
            .collect();
        Ok(())
    }
}
