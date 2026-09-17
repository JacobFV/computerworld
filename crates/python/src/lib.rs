//! Object conversion and ownership only; simulation lives in the Rust facade.
use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyBytes, PyDict},
};
use serde::{de::DeserializeOwned, Serialize};
use std::{cell::RefCell, rc::Rc};
fn err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
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
#[pyclass(unsendable)]
struct World {
    inner: Rc<RefCell<cw::World>>,
}
#[pymethods]
impl World {
    #[new]
    #[pyo3(signature = (definition, seed=0))]
    fn new(definition: &Bound<'_, PyAny>, seed: u64) -> PyResult<Self> {
        Ok(Self {
            inner: Rc::new(RefCell::new(
                cw::World::new(decode(definition)?, seed).map_err(err)?,
            )),
        })
    }
    fn environment(&self, config: &Bound<'_, PyAny>) -> PyResult<Environment> {
        let config = decode(config)?;
        let session = self.inner.borrow_mut().environment(config).map_err(err)?;
        Ok(Environment {
            inner: self.inner.clone(),
            session,
        })
    }
    fn session(&self, id: &str) -> PyResult<Environment> {
        self.inner.borrow().validate_session(id).map_err(err)?;
        Ok(Environment {
            inner: self.inner.clone(),
            session: id.to_owned(),
        })
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            inner: self.inner.borrow().snapshot(),
        }
    }
    fn restore(&self, snapshot: &Snapshot) -> PyResult<()> {
        self.inner
            .borrow_mut()
            .restore(&snapshot.inner)
            .map_err(err)
    }
    fn fork(&self, snapshot: &Snapshot) -> PyResult<Self> {
        Ok(Self {
            inner: Rc::new(RefCell::new(
                self.inner.borrow().fork(&snapshot.inner).map_err(err)?,
            )),
        })
    }
    #[pyo3(signature = (seed=0))]
    fn reset(&self, seed: u64) -> PyResult<()> {
        self.inner.borrow_mut().reset(seed).map_err(err)
    }
    fn export_snapshot(&self) -> PyResult<String> {
        self.inner.borrow().export_snapshot().map_err(err)
    }
    fn import_snapshot(&self, json: &str) -> PyResult<()> {
        self.inner.borrow_mut().import_snapshot(json).map_err(err)
    }
    fn state_hash(&self) -> PyResult<String> {
        self.inner.borrow().state_hash().map_err(err)
    }
    fn trajectory(&self, py: Python<'_>) -> PyResult<PyObject> {
        encode(py, &self.inner.borrow().trajectory())
    }
    fn definition(&self, py: Python<'_>) -> PyResult<PyObject> {
        encode(py, self.inner.borrow().definition())
    }
    fn add_computer(
        &self,
        computer: &Bound<'_, PyAny>,
        node: &Bound<'_, PyAny>,
        links: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.inner
            .borrow_mut()
            .add_computer(decode(computer)?, decode(node)?, decode(links)?)
            .map_err(err)
    }
    fn remove_computer(&self, id: &str) -> PyResult<()> {
        self.inner.borrow_mut().remove_computer(id).map_err(err)
    }
    fn inspect(&self, py: Python<'_>) -> PyResult<PyObject> {
        encode(py, &self.inner.borrow().inspect())
    }
}
#[pyclass(unsendable)]
struct Snapshot {
    inner: cw::Snapshot,
}
#[pyclass(unsendable)]
struct Environment {
    inner: Rc<RefCell<cw::World>>,
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
        let result = self
            .inner
            .borrow_mut()
            .step(&self.session, actions)
            .map_err(err)?;
        encode(py, &result)
    }
    fn observe(&self, py: Python<'_>) -> PyResult<PyObject> {
        encode(
            py,
            &self.inner.borrow().observe(&self.session).map_err(err)?,
        )
    }
    #[pyo3(signature = (width=1024, height=768))]
    fn scene(&self, py: Python<'_>, width: u32, height: u32) -> PyResult<PyObject> {
        encode(
            py,
            &self
                .inner
                .borrow()
                .scene(&self.session, width, height)
                .map_err(err)?,
        )
    }
    #[pyo3(signature = (width=1024, height=768))]
    fn render(&self, py: Python<'_>, width: u32, height: u32) -> PyResult<PyObject> {
        let frame = self
            .inner
            .borrow_mut()
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
