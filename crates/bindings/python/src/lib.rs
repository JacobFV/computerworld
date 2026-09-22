//! Object conversion and ownership only; simulation lives in the Rust facade.
//!
//! The handles are safe to hand to another thread. A [`World`] and every [`Environment`]
//! minted from it share one `Mutex` around the Rust world, so calls from several threads
//! are serialized rather than racing, and a call that arrives while another is running
//! waits for it. What is *not* safe is two threads driving one world expecting two
//! independent simulations: the world is one state machine, and the order its actions
//! land in is the order the threads acquired the lock. See `docs/python.md`.
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyDict},
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};
fn err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}
/// The shared world, or a Python exception explaining why it is unusable. A panic
/// inside the simulation poisons the lock; every later call then raises instead of
/// aborting the interpreter, which is what an `unsendable` class used to do.
fn hold(world: &Arc<Mutex<cw::World>>) -> PyResult<MutexGuard<'_, cw::World>> {
    world.lock().map_err(|_| {
        PyRuntimeError::new_err(
            "the simulation panicked during an earlier call and this world can no longer \
             be used; build a new World, or restore one from an exported snapshot",
        )
    })
}
fn decode<T: DeserializeOwned>(value: &Bound<'_, PyAny>) -> PyResult<T> {
    let json: String = value
        .py()
        .import("json")?
        .call_method1("dumps", (value,))?
        .extract()?;
    serde_json::from_str(&json).map_err(err)
}
fn encode(py: Python<'_>, value: &impl Serialize) -> PyResult<PyObject> {
    Ok(py
        .import("json")?
        .call_method1("loads", (serde_json::to_string(value).map_err(err)?,))?
        .unbind())
}
#[pyclass]
struct World {
    inner: Arc<Mutex<cw::World>>,
}
#[pymethods]
impl World {
    #[new]
    #[pyo3(signature = (definition, seed=0))]
    fn new(definition: &Bound<'_, PyAny>, seed: u64) -> PyResult<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(
                cw::World::new(decode(definition)?, seed).map_err(err)?,
            )),
        })
    }
    fn environment(&self, config: &Bound<'_, PyAny>) -> PyResult<Environment> {
        let config = decode(config)?;
        let session = hold(&self.inner)?.environment(config).map_err(err)?;
        Ok(Environment {
            inner: self.inner.clone(),
            session,
        })
    }
    fn session(&self, id: &str) -> PyResult<Environment> {
        hold(&self.inner)?.validate_session(id).map_err(err)?;
        Ok(Environment {
            inner: self.inner.clone(),
            session: id.to_owned(),
        })
    }
    fn snapshot(&self) -> PyResult<Snapshot> {
        Ok(Snapshot {
            inner: hold(&self.inner)?.snapshot(),
        })
    }
    fn restore(&self, snapshot: &Snapshot) -> PyResult<()> {
        hold(&self.inner)?.restore(&snapshot.inner).map_err(err)
    }
    fn fork(&self, snapshot: &Snapshot) -> PyResult<Self> {
        let forked = hold(&self.inner)?.fork(&snapshot.inner).map_err(err)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(forked)),
        })
    }
    #[pyo3(signature = (seed=0))]
    fn reset(&self, seed: u64) -> PyResult<()> {
        hold(&self.inner)?.reset(seed).map_err(err)
    }
    fn export_snapshot(&self) -> PyResult<String> {
        hold(&self.inner)?.export_snapshot().map_err(err)
    }
    fn import_snapshot(&self, json: &str) -> PyResult<()> {
        hold(&self.inner)?.import_snapshot(json).map_err(err)
    }
    fn state_hash(&self) -> PyResult<String> {
        hold(&self.inner)?.state_hash().map_err(err)
    }
    fn trajectory(&self, py: Python<'_>) -> PyResult<PyObject> {
        let trajectory = hold(&self.inner)?.trajectory().clone();
        encode(py, &trajectory)
    }
    fn definition(&self, py: Python<'_>) -> PyResult<PyObject> {
        let definition = hold(&self.inner)?.definition().clone();
        encode(py, &definition)
    }
    fn add_computer(
        &self,
        computer: &Bound<'_, PyAny>,
        node: &Bound<'_, PyAny>,
        links: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let (computer, node, links) = (decode(computer)?, decode(node)?, decode(links)?);
        hold(&self.inner)?
            .add_computer(computer, node, links)
            .map_err(err)
    }
    fn remove_computer(&self, id: &str) -> PyResult<()> {
        hold(&self.inner)?.remove_computer(id).map_err(err)
    }
    fn inspect(&self, py: Python<'_>) -> PyResult<PyObject> {
        let inspected = hold(&self.inner)?.inspect();
        encode(py, &inspected)
    }
}
#[pyclass]
struct Snapshot {
    inner: cw::Snapshot,
}
#[pyclass]
struct Environment {
    inner: Arc<Mutex<cw::World>>,
    session: String,
}
#[pymethods]
impl Environment {
    #[getter]
    fn id(&self) -> &str {
        &self.session
    }

    fn step(&self, py: Python<'_>, actions: &Bound<'_, PyAny>) -> PyResult<PyObject> {
        let actions = decode(actions)?;
        let result = hold(&self.inner)?
            .step(&self.session, actions)
            .map_err(err)?;
        encode(py, &result)
    }
    fn observe(&self, py: Python<'_>) -> PyResult<PyObject> {
        let observation = hold(&self.inner)?.observe(&self.session).map_err(err)?;
        encode(py, &observation)
    }
    #[pyo3(signature = (width=1024, height=768))]
    fn scene(&self, py: Python<'_>, width: u32, height: u32) -> PyResult<PyObject> {
        let scene = hold(&self.inner)?
            .scene(&self.session, width, height)
            .map_err(err)?;
        encode(py, &scene)
    }
    #[pyo3(signature = (width=1024, height=768))]
    fn render(&self, py: Python<'_>, width: u32, height: u32) -> PyResult<PyObject> {
        let frame = hold(&self.inner)?
            .render(&self.session, width, height)
            .map_err(err)?;
        let result = PyDict::new(py);
        result.set_item("width", frame.width)?;
        result.set_item("height", frame.height)?;
        result.set_item("rgba", PyBytes::new(py, &frame.rgba))?;
        Ok(result.into_any().unbind())
    }
}
#[pymodule]
fn computerworld(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // The classes are only sound to share across threads because the world they hold
    // is; if that ever stops being true this line stops compiling rather than turning
    // into an abort at runtime.
    const fn shareable<T: Send>() {}
    shareable::<cw::World>();
    shareable::<cw::Snapshot>();
    m.add(
        "__version__",
        env!("CARGO_PKG_VERSION").replace("-alpha.", "a"),
    )?;
    m.add("engine_version", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<World>()?;
    m.add_class::<Environment>()?;
    m.add_class::<Snapshot>()?;
    Ok(())
}
