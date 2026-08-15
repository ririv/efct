use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyModule, PyTuple};

use crate::certificate::{CallableKind, Certificate};
use crate::contract::{ValueTypes, bind_arguments, matches_type};
use crate::dependency::{Environment, SealedExecution, seal};
use crate::dependency_verifier::verify_dependencies;
use crate::error::{RuntimeFailure, contract_error, integrity_error};

struct VerifiedRuntime {
    code: Py<PyAny>,
    module: Py<PyModule>,
    certificate: Certificate,
    state: Mutex<RuntimeState>,
}

static ENVIRONMENT: PyOnceLock<Environment> = PyOnceLock::new();
static VALUE_TYPES: PyOnceLock<ValueTypes> = PyOnceLock::new();

enum RuntimeState {
    Active(ExecutionState),
    Revoked(String),
}

enum ExecutionState {
    Unsealed,
    Sealed(SealedExecution),
}

#[pyclass(frozen, module = "efct.runtime")]
pub struct PureFunction {
    runtime: VerifiedRuntime,
}

#[pyclass(frozen, module = "efct.runtime")]
pub struct EffectFunction {
    runtime: VerifiedRuntime,
}

impl VerifiedRuntime {
    fn new(
        code: Py<PyAny>,
        module: Py<PyModule>,
        certificate: Certificate,
        expected_kind: ExpectedKind,
    ) -> PyResult<Self> {
        if !expected_kind.accepts(certificate.callable_kind) {
            return Err(PyTypeError::new_err(
                "The runtime wrapper does not match the certificate callable kind",
            ));
        }
        Ok(Self {
            code,
            module,
            certificate,
            state: Mutex::new(RuntimeState::Active(ExecutionState::Unsealed)),
        })
    }

    fn certificate(&self) -> &Certificate {
        &self.certificate
    }

    fn certificate_object(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &*self.lock_state()? {
            RuntimeState::Active(_) => Ok(self.certificate.object.clone_ref(py)),
            RuntimeState::Revoked(reason) => Err(integrity_error(
                py,
                format!("The Efct certificate has been revoked: {reason}"),
            )),
        }
    }

    fn call(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let sealed = self.sealed(py)?;
        if let Err(failure) =
            verify_dependencies(py, &self.module, environment(py)?, &sealed.dependencies)
        {
            return Err(self.handle_failure(py, failure));
        }
        let values = bind_arguments(py, &self.certificate, args, kwargs)?;
        let mut effect_bindings = HashMap::new();
        for ((name, expected), value) in self
            .certificate
            .parameter_names
            .iter()
            .zip(&self.certificate.parameter_types)
            .zip(&values)
        {
            if !matches_type(
                py,
                value.bind(py),
                expected,
                value_types(py)?,
                &mut effect_bindings,
            )? {
                return Err(contract_error(
                    py,
                    format!(
                        "Argument {name} does not satisfy exact type {}",
                        expected.format(py)?
                    ),
                ));
            }
        }
        let call_args = PyTuple::new(py, values.iter().map(|value| value.bind(py)))?;
        let result = sealed.function.bind(py).call1(call_args)?;
        if !matches_type(
            py,
            &result,
            &self.certificate.return_type,
            value_types(py)?,
            &mut effect_bindings,
        )? {
            return Err(self.revoke(
                py,
                format!(
                    "The return value does not satisfy exact type {}",
                    self.certificate.return_type.format(py)?
                ),
            ));
        }
        Ok(result.unbind())
    }

    fn sealed(&self, py: Python<'_>) -> PyResult<SealedExecution> {
        {
            let state = self.lock_state()?;
            match &*state {
                RuntimeState::Revoked(reason) => {
                    return Err(integrity_error(
                        py,
                        format!("The Efct certificate has been revoked: {reason}"),
                    ));
                }
                RuntimeState::Active(ExecutionState::Sealed(sealed)) => {
                    return Ok(sealed.clone_ref(py));
                }
                RuntimeState::Active(ExecutionState::Unsealed) => {}
            }
        }
        let sealed = match seal(
            py,
            &self.code,
            &self.module,
            &self.certificate,
            environment(py)?,
            value_types(py)?,
        ) {
            Ok(sealed) => sealed,
            Err(failure) => return Err(self.handle_failure(py, failure)),
        };
        let snapshot = sealed.clone_ref(py);
        let mut state = self.lock_state()?;
        match &*state {
            RuntimeState::Revoked(reason) => Err(integrity_error(
                py,
                format!("The Efct certificate has been revoked: {reason}"),
            )),
            RuntimeState::Active(ExecutionState::Sealed(existing)) => Ok(existing.clone_ref(py)),
            RuntimeState::Active(ExecutionState::Unsealed) => {
                *state = RuntimeState::Active(ExecutionState::Sealed(sealed));
                Ok(snapshot)
            }
        }
    }

    fn handle_failure(&self, py: Python<'_>, failure: RuntimeFailure) -> PyErr {
        match failure {
            RuntimeFailure::Integrity(reason) => self.revoke(py, reason),
            RuntimeFailure::Python(error) => error,
        }
    }

    fn revoke(&self, py: Python<'_>, reason: String) -> PyErr {
        if let Ok(mut state) = self.state.lock() {
            *state = RuntimeState::Revoked(reason.clone());
        }
        integrity_error(py, reason)
    }

    fn lock_state(&self) -> PyResult<MutexGuard<'_, RuntimeState>> {
        self.state
            .lock()
            .map_err(|_| PyRuntimeError::new_err("The verified runtime lock is poisoned"))
    }
}

pub(crate) fn initialize(py: Python<'_>) -> PyResult<()> {
    ENVIRONMENT.get_or_try_init(py, || Environment::load(py))?;
    VALUE_TYPES.get_or_try_init(py, || ValueTypes::load(py))?;
    Ok(())
}

fn environment(py: Python<'_>) -> PyResult<&'static Environment> {
    ENVIRONMENT.get_or_try_init(py, || Environment::load(py))
}

fn value_types(py: Python<'_>) -> PyResult<&'static ValueTypes> {
    VALUE_TYPES.get_or_try_init(py, || ValueTypes::load(py))
}

enum ExpectedKind {
    Pure,
    Effect,
}

impl ExpectedKind {
    fn accepts(&self, kind: CallableKind) -> bool {
        match self {
            Self::Pure => matches!(kind, CallableKind::InferredPure | CallableKind::BoundedPure),
            Self::Effect => matches!(
                kind,
                CallableKind::InferredEffect | CallableKind::BoundedEffect
            ),
        }
    }
}

#[pymethods]
impl PureFunction {
    #[new]
    fn rejected_construction(py: Python<'_>, _value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Err(construction_error(
            py,
            "PureFunction can only be constructed by the verifier",
        )?)
    }

    #[getter]
    fn certificate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.runtime.certificate_object(py)
    }

    #[getter]
    fn __name__(&self) -> &str {
        &self.runtime.certificate.function_name
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.runtime.call(py, args, kwargs)
    }
}

#[pymethods]
impl EffectFunction {
    #[new]
    fn rejected_construction(py: Python<'_>, _value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Err(construction_error(
            py,
            "EffectFunction can only be constructed by the verifier",
        )?)
    }

    #[getter]
    fn certificate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.runtime.certificate_object(py)
    }

    #[getter]
    fn __name__(&self) -> &str {
        &self.runtime.certificate.function_name
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.runtime.call(py, args, kwargs)
    }
}

impl PureFunction {
    pub(crate) fn native_certificate(&self) -> &Certificate {
        self.runtime.certificate()
    }
}

impl EffectFunction {
    pub(crate) fn native_certificate(&self) -> &Certificate {
        self.runtime.certificate()
    }
}

pub(crate) fn with_verified_certificate<T>(
    value: &Bound<'_, PyAny>,
    inspect: impl FnOnce(&Certificate) -> T,
) -> Option<T> {
    if let Ok(wrapper) = value.extract::<PyRef<'_, PureFunction>>() {
        return Some(inspect(wrapper.native_certificate()));
    }
    let wrapper = value.extract::<PyRef<'_, EffectFunction>>().ok()?;
    Some(inspect(wrapper.native_certificate()))
}

pub fn create_pure_function(
    code: Py<PyAny>,
    module: Py<PyModule>,
    certificate: Certificate,
) -> PyResult<PureFunction> {
    Ok(PureFunction {
        runtime: VerifiedRuntime::new(code, module, certificate, ExpectedKind::Pure)?,
    })
}

pub fn create_effect_function(
    code: Py<PyAny>,
    module: Py<PyModule>,
    certificate: Certificate,
) -> PyResult<EffectFunction> {
    Ok(EffectFunction {
        runtime: VerifiedRuntime::new(code, module, certificate, ExpectedKind::Effect)?,
    })
}

fn construction_error(py: Python<'_>, message: &str) -> PyResult<PyErr> {
    let localized = py
        .import("efct.i18n")
        .and_then(|module| module.getattr("localize_error_text"))
        .and_then(|function| function.call1((message,)))
        .and_then(|value| value.extract::<String>())?;
    Ok(PyTypeError::new_err(localized))
}
