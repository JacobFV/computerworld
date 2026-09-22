//! Thin JavaScript bindings. All semantics execute in the shared Rust facade.
use serde::{de::DeserializeOwned, Serialize};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;

fn error(e: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&e.to_string()).into()
}
fn seed_value(value: JsValue) -> Result<u64, JsValue> {
    if let Some(text) = value.as_string() {
        text.parse::<u64>().map_err(error)
    } else {
        serde_wasm_bindgen::from_value(value).map_err(error)
    }
}
fn decode<T: DeserializeOwned>(v: JsValue) -> Result<T, JsValue> {
    serde_json::from_value(decode_value(&v, 0)?).map_err(error)
}
/// Exact language conversion only: preserve JSON's integer semantics and reject
/// JavaScript values that have no portable serialized counterpart.
fn decode_value(value: &JsValue, depth: usize) -> Result<serde_json::Value, JsValue> {
    use serde_json::{Number, Value};
    if depth > 128 {
        return Err(error("value exceeds maximum nesting depth 128"));
    }
    if value.is_null() {
        return Ok(Value::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = value.as_string() {
        return Ok(Value::String(value));
    }
    if value.is_bigint() {
        let bigint = value.unchecked_ref::<js_sys::BigInt>();
        let text = bigint
            .to_string(10)?
            .as_string()
            .ok_or_else(|| error("invalid BigInt"))?;
        let number = if text.starts_with('-') {
            Number::from(text.parse::<i64>().map_err(error)?)
        } else {
            Number::from(text.parse::<u64>().map_err(error)?)
        };
        return Ok(Value::Number(number));
    }
    if let Some(value) = value.as_f64() {
        if !value.is_finite() {
            return Err(error("non-finite Number is not serializable"));
        }
        if value.fract() == 0.0 {
            if value.abs() > 9_007_199_254_740_991.0 {
                return Err(error(
                    "unsafe integral Number; use BigInt for exact integers",
                ));
            }
            return Ok(Value::Number(if value < 0.0 {
                Number::from(value as i64)
            } else {
                Number::from(value as u64)
            }));
        }
        return Ok(Value::Number(
            Number::from_f64(value).ok_or_else(|| error("invalid Number"))?,
        ));
    }
    if js_sys::Array::is_array(value) {
        let array = value.unchecked_ref::<js_sys::Array>();
        let mut values = Vec::with_capacity(array.length() as usize);
        for index in 0..array.length() {
            values.push(decode_value(
                &js_sys::Reflect::get(value, &JsValue::from_f64(index.into()))?,
                depth + 1,
            )?);
        }
        return Ok(Value::Array(values));
    }
    if value.is_object() {
        let prototype = js_sys::Reflect::get_prototype_of(value)?;
        let plain_prototype = js_sys::Object::get_prototype_of(&js_sys::Object::new());
        if !prototype.is_null() && !js_sys::Object::is(&prototype, &plain_prototype) {
            return Err(error("expected a plain object or array"));
        }
        let mut values = serde_json::Map::new();
        for key in js_sys::Reflect::own_keys(value)?.iter() {
            let name = key
                .as_string()
                .ok_or_else(|| error("symbol keys are not serializable"))?;
            values.insert(
                name,
                decode_value(&js_sys::Reflect::get(value, &key)?, depth + 1)?,
            );
        }
        return Ok(Value::Object(values));
    }
    Err(error(
        "undefined, functions and symbols are not serializable",
    ))
}
fn encode<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    encode_value(&serde_json::to_value(v).map_err(error)?)
}
fn encode_value(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    use serde_json::Value;
    Ok(match value {
        Value::Null => JsValue::NULL,
        Value::Bool(v) => JsValue::from_bool(*v),
        Value::String(v) => JsValue::from_str(v),
        Value::Number(v) => {
            const MAX_SAFE: u64 = 9_007_199_254_740_991;
            let large = v.as_u64().is_some_and(|n| n > MAX_SAFE)
                || v.as_i64().is_some_and(|n| n < -(MAX_SAFE as i64));
            if large {
                js_sys::BigInt::new(&JsValue::from_str(&v.to_string()))?.into()
            } else {
                JsValue::from_f64(v.as_f64().ok_or_else(|| error("invalid number"))?)
            }
        }
        Value::Array(values) => {
            let array = js_sys::Array::new();
            for value in values {
                array.push(&encode_value(value)?);
            }
            array.into()
        }
        Value::Object(values) => {
            // Null prototype preserves arbitrary serialized keys, including
            // "__proto__", as ordinary data without prototype setters.
            let object = js_sys::Object::create(JsValue::NULL.unchecked_ref::<js_sys::Object>());
            for (key, value) in values {
                js_sys::Reflect::set(&object, &JsValue::from_str(key), &encode_value(value)?)?;
            }
            object.into()
        }
    })
}

/// Privileged owner handle. Give agents only the Environment returned by environment().
#[wasm_bindgen(js_name = World)]
pub struct JsWorld {
    inner: Rc<RefCell<computerworld::World>>,
}
#[wasm_bindgen(js_class = World)]
impl JsWorld {
    #[wasm_bindgen(constructor)]
    pub fn new(definition: JsValue, seed: JsValue) -> Result<JsWorld, JsValue> {
        Ok(Self {
            inner: Rc::new(RefCell::new(
                computerworld::World::new(decode(definition)?, seed_value(seed)?).map_err(error)?,
            )),
        })
    }
    pub fn environment(&self, config: JsValue) -> Result<JsEnvironment, JsValue> {
        let config = decode(config)?;
        let session = self.inner.borrow_mut().environment(config).map_err(error)?;
        Ok(JsEnvironment {
            inner: self.inner.clone(),
            session,
        })
    }
    /// Reconnect an actor stored in a restored/forked checkpoint.
    pub fn session(&self, id: &str) -> Result<JsEnvironment, JsValue> {
        self.inner.borrow().validate_session(id).map_err(error)?;
        Ok(JsEnvironment {
            inner: self.inner.clone(),
            session: id.to_owned(),
        })
    }
    pub fn snapshot(&self) -> JsSnapshot {
        JsSnapshot {
            inner: self.inner.borrow().snapshot(),
        }
    }
    pub fn restore(&self, snapshot: &JsSnapshot) -> Result<(), JsValue> {
        self.inner
            .borrow_mut()
            .restore(&snapshot.inner)
            .map_err(error)
    }
    pub fn fork(&self, snapshot: &JsSnapshot) -> Result<JsWorld, JsValue> {
        Ok(Self {
            inner: Rc::new(RefCell::new(
                self.inner.borrow().fork(&snapshot.inner).map_err(error)?,
            )),
        })
    }
    pub fn reset(&self, seed: JsValue) -> Result<(), JsValue> {
        self.inner
            .borrow_mut()
            .reset(seed_value(seed)?)
            .map_err(error)
    }
    #[wasm_bindgen(js_name = exportSnapshot)]
    pub fn export_snapshot(&self) -> Result<String, JsValue> {
        self.inner.borrow().export_snapshot().map_err(error)
    }
    #[wasm_bindgen(js_name = importSnapshot)]
    pub fn import_snapshot(&self, json: &str) -> Result<(), JsValue> {
        self.inner.borrow_mut().import_snapshot(json).map_err(error)
    }
    #[wasm_bindgen(js_name = stateHash)]
    pub fn state_hash(&self) -> Result<String, JsValue> {
        self.inner.borrow().state_hash().map_err(error)
    }
    pub fn trajectory(&self) -> Result<JsValue, JsValue> {
        encode(&self.inner.borrow().trajectory())
    }
    pub fn definition(&self) -> Result<JsValue, JsValue> {
        encode(self.inner.borrow().definition())
    }
    /// Owner-only topology mutation. Wiring is explicit; no implicit host access.
    #[wasm_bindgen(js_name = addComputer)]
    pub fn add_computer(
        &self,
        computer: JsValue,
        node: JsValue,
        links: JsValue,
    ) -> Result<(), JsValue> {
        self.inner
            .borrow_mut()
            .add_computer(decode(computer)?, decode(node)?, decode(links)?)
            .map_err(error)
    }
    #[wasm_bindgen(js_name = removeComputer)]
    pub fn remove_computer(&self, id: &str) -> Result<(), JsValue> {
        self.inner.borrow_mut().remove_computer(id).map_err(error)
    }
    pub fn inspect(&self) -> Result<JsValue, JsValue> {
        encode(&self.inner.borrow().inspect())
    }
}

#[wasm_bindgen(js_name = Snapshot)]
pub struct JsSnapshot {
    inner: computerworld::Snapshot,
}

/// Restricted actor handle: no privileged inspection, topology export, or snapshots.
#[wasm_bindgen(js_name = Environment)]
pub struct JsEnvironment {
    inner: Rc<RefCell<computerworld::World>>,
    session: String,
}
#[wasm_bindgen(js_class = Environment)]
impl JsEnvironment {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.session.clone()
    }

    pub fn step(&self, actions: JsValue) -> Result<JsValue, JsValue> {
        let actions = decode(actions)?;
        encode(
            &self
                .inner
                .borrow_mut()
                .step(&self.session, actions)
                .map_err(error)?,
        )
    }
    pub fn observe(&self) -> Result<JsValue, JsValue> {
        encode(&self.inner.borrow().observe(&self.session).map_err(error)?)
    }
    pub fn scene(&self, width: u32, height: u32) -> Result<JsValue, JsValue> {
        encode(
            &self
                .inner
                .borrow()
                .scene(&self.session, width, height)
                .map_err(error)?,
        )
    }
    pub fn render(&self, width: u32, height: u32) -> Result<JsFrame, JsValue> {
        let frame = self
            .inner
            .borrow_mut()
            .render(&self.session, width, height)
            .map_err(error)?;
        Ok(JsFrame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        })
    }
    /// Render into a buffer the caller already holds — an `ImageData`'s `data`, in
    /// practice — so the pixels cross into JavaScript exactly once. `render` copies the
    /// frame out of the renderer, then again into a fresh `Uint8Array`, and a canvas
    /// wants a `Uint8ClampedArray` after that; a screen redrawn on every pointer move is
    /// three copies of four megabytes that nothing reads.
    #[wasm_bindgen(js_name = renderInto)]
    pub fn render_into(
        &self,
        width: u32,
        height: u32,
        out: &js_sys::Uint8ClampedArray,
    ) -> Result<(), JsValue> {
        self.inner
            .borrow_mut()
            .render_with(&self.session, width, height, |frame| {
                if out.length() as usize != frame.rgba.len() {
                    return Err(error(format!(
                        "the buffer holds {} bytes; a {width}x{height} frame is {}",
                        out.length(),
                        frame.rgba.len()
                    )));
                }
                out.copy_from(&frame.rgba);
                Ok(())
            })
            .map_err(error)?
    }
}
#[wasm_bindgen(js_name = Frame)]
pub struct JsFrame {
    pub width: u32,
    pub height: u32,
    rgba: Vec<u8>,
}
#[wasm_bindgen(js_class = Frame)]
impl JsFrame {
    #[wasm_bindgen(getter)]
    pub fn rgba(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.rgba.as_slice())
    }
}
#[wasm_bindgen(js_name = createWorld)]
pub fn create_world(definition: JsValue, seed: JsValue) -> Result<JsWorld, JsValue> {
    JsWorld::new(definition, seed)
}

/// Standalone retained renderer for synthetic applications and measurement.
#[wasm_bindgen(js_name = SceneRenderer)]
pub struct JsSceneRenderer {
    scene: Option<cw_scene::Scene>,
    renderer: cw_render::Renderer,
}
#[wasm_bindgen(js_class = SceneRenderer)]
impl JsSceneRenderer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            scene: None,
            renderer: cw_render::Renderer::new(),
        }
    }
    pub fn render(&mut self, scene: JsValue) -> Result<JsFrame, JsValue> {
        let scene: cw_scene::Scene = serde_wasm_bindgen::from_value(scene).map_err(error)?;
        scene.validate().map_err(error)?;
        let frame = self.renderer.render(&scene);
        self.scene = Some(scene);
        Ok(JsFrame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        })
    }
    pub fn patch(&mut self, patch: JsValue) -> Result<JsFrame, JsValue> {
        let scene = self
            .scene
            .as_mut()
            .ok_or_else(|| error("render a scene before applying patches"))?;
        let damage = scene
            .patch(serde_wasm_bindgen::from_value(patch).map_err(error)?)
            .map_err(error)?;
        let frame = self.renderer.render_incremental(scene, &damage);
        Ok(JsFrame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.clone(),
        })
    }
}
impl Default for JsSceneRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Install one file of the CJK/emoji font pack (`fonts/<file>` beside this module).
/// The Wasm build does not embed those faces; until a file is installed its glyphs
/// draw as `.notdef` boxes, while layout is already final. Bytes are identified by
/// SHA-256, so only the exact files this build was made with are accepted, and every
/// renderer drops its cached text on the next frame. Returns the file name.
#[wasm_bindgen(js_name = installFont)]
pub fn install_font(bytes: &[u8]) -> Result<String, JsValue> {
    cw_render::install_font(bytes)
        .map(|file| file.file.to_owned())
        .map_err(error)
}

/// The font pack: every file with its SHA-256 and size, which are installed, and
/// which renderers have needed but not had (fetch exactly those, then re-render).
#[wasm_bindgen(js_name = fontPackStatus)]
pub fn font_pack_status() -> Result<JsValue, JsValue> {
    let status = cw_render::font_pack_status();
    let files: Vec<serde_json::Value> = cw_render::FONT_PACK
        .iter()
        .map(|file| {
            serde_json::json!({
                "file": file.file,
                "path": format!("fonts/{}", file.file),
                "sha256": file.sha256,
                "bytes": file.bytes,
                "installed": status.installed.contains(&file.file),
            })
        })
        .collect();
    encode(&serde_json::json!({
        "files": files,
        "installed": status.installed,
        "missing": status.missing,
    }))
}

/// Version of the canonical engine embedded in this binding.
#[wasm_bindgen(js_name = engineVersion)]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
