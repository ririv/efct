use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

#[derive(Clone)]
pub(crate) struct VerifiedSource {
    raw: Vec<u8>,
    filename: String,
    is_package: bool,
}

impl VerifiedSource {
    pub(crate) fn new(raw: Vec<u8>, filename: String, is_package: bool) -> Self {
        Self {
            raw,
            filename,
            is_package,
        }
    }
}

enum PreviousModule {
    Missing,
    Present(Py<PyAny>),
}

#[pyclass(frozen, name = "_VerifiedSourceFinder", module = "efct._core")]
pub struct VerifiedSourceFinder {
    sources: BTreeMap<String, VerifiedSource>,
}

#[pymethods]
impl VerifiedSourceFinder {
    #[pyo3(signature = (fullname, _path=None, _target=None))]
    fn find_spec(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        fullname: &str,
        _path: Option<&Bound<'_, PyAny>>,
        _target: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let Some(source) = slf.sources.get(fullname) else {
            return Ok(None);
        };
        let kwargs = PyDict::new(py);
        kwargs.set_item("is_package", source.is_package)?;
        let spec = py
            .import("importlib.util")?
            .getattr("spec_from_loader")?
            .call((fullname, slf.into_pyobject(py)?), Some(&kwargs))?;
        Ok(Some(spec.unbind()))
    }

    fn create_module(&self, _spec: &Bound<'_, PyAny>) -> Option<Py<PyAny>> {
        None
    }

    fn exec_module(&self, py: Python<'_>, module: &Bound<'_, PyAny>) -> PyResult<()> {
        let name = module.getattr("__name__")?.extract::<String>()?;
        let code = self.compile(py, &name)?;
        py.import("builtins")?
            .getattr("exec")?
            .call1((code, module.getattr("__dict__")?))?;
        Ok(())
    }

    fn get_code(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        Ok(self.compile(py, fullname)?.unbind())
    }

    fn get_filename(&self, fullname: &str) -> PyResult<String> {
        Ok(self.source(fullname)?.filename.clone())
    }

    fn is_package(&self, fullname: &str) -> PyResult<bool> {
        Ok(self.source(fullname)?.is_package)
    }
}

impl VerifiedSourceFinder {
    fn source(&self, fullname: &str) -> PyResult<&VerifiedSource> {
        self.sources.get(fullname).ok_or_else(|| {
            pyo3::exceptions::PyImportError::new_err(format!(
                "Verified source for module {fullname} is unavailable"
            ))
        })
    }

    fn compile<'py>(&self, py: Python<'py>, fullname: &str) -> PyResult<Bound<'py, PyAny>> {
        let source = self.source(fullname)?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("dont_inherit", true)?;
        py.import("builtins")?.getattr("compile")?.call(
            (
                PyBytes::new(py, &source.raw),
                source.filename.as_str(),
                "exec",
            ),
            Some(&kwargs),
        )
    }
}

pub(crate) fn execute(
    py: Python<'_>,
    entry_module: &str,
    sources: BTreeMap<String, VerifiedSource>,
    arguments: &Bound<'_, PyList>,
) -> PyResult<()> {
    if !sources.contains_key(entry_module) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "The verified entry module is missing from the source set",
        ));
    }
    let source_names = sources.keys().cloned().collect::<Vec<_>>();
    let finder = Py::new(py, VerifiedSourceFinder { sources })?;
    let sys = py.import("sys")?;
    let modules = sys.getattr("modules")?.cast_into::<PyDict>()?;
    let previous_modules = remove_previous_modules(&modules, &source_names)?;
    let meta_path = sys.getattr("meta_path")?.cast_into::<PyList>()?;
    meta_path.insert(0, &finder)?;
    let original_arguments = sys.getattr("argv")?.unbind();
    sys.setattr("argv", arguments)?;

    let kwargs = PyDict::new(py);
    kwargs.set_item("run_name", "__main__")?;
    kwargs.set_item("alter_sys", true)?;
    let result = py
        .import("runpy")?
        .getattr("run_module")?
        .call((entry_module,), Some(&kwargs));

    let restore_arguments = sys.setattr("argv", original_arguments);
    let remove_finder = meta_path.call_method1("remove", (&finder,));
    let restore_modules = restore_previous_modules(&modules, previous_modules);
    result?;
    restore_arguments?;
    remove_finder?;
    restore_modules?;
    Ok(())
}

fn remove_previous_modules(
    modules: &Bound<'_, PyDict>,
    names: &[String],
) -> PyResult<BTreeMap<String, PreviousModule>> {
    let mut previous = BTreeMap::new();
    for name in names {
        let state = match modules.get_item(name)? {
            Some(module) => {
                modules.del_item(name)?;
                PreviousModule::Present(module.unbind())
            }
            None => PreviousModule::Missing,
        };
        previous.insert(name.clone(), state);
    }
    Ok(previous)
}

fn restore_previous_modules(
    modules: &Bound<'_, PyDict>,
    previous: BTreeMap<String, PreviousModule>,
) -> PyResult<()> {
    for (name, state) in previous {
        if modules.contains(&name)? {
            modules.del_item(&name)?;
        }
        if let PreviousModule::Present(module) = state {
            modules.set_item(name, module)?;
        }
    }
    Ok(())
}
