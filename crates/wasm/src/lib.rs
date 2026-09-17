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
    // JavaScript has one Number type. Deserialize through the canonical JSON
    // boundary so integral payload values remain JSON integers, exactly like
    // native/Python inputs (serde-wasm-bindgen's deserialize_any uses f64).
    let json = js_sys::JSON::stringify(&v)?;
    let text = json
        .as_string()
        .ok_or_else(|| error("expected a JSON value"))?;
    serde_json::from_str(&text).map_err(error)
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
