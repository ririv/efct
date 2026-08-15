use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

use crate::dependency::{Dependency, Environment, ExternalBoundarySeal};
use crate::error::RuntimeFailure;

pub fn verify_dependencies(
    py: Python<'_>,
    module: &Py<PyModule>,
    environment: &Environment,
    dependencies: &[Dependency],
) -> Result<(), RuntimeFailure> {
    let bindings = module.bind(py).dict();
    for dependency in dependencies {
        match dependency {
            Dependency::Module { name, value } => {
                verify_binding(
                    &bindings,
                    name,
                    value,
                    "Runtime dependency",
                    "has been rebound",
                )?;
            }
            Dependency::Builtin { name, value } => {
                let actual = environment.builtins().bind(py).getattr(name.as_str()).ok();
                if !actual.is_some_and(|actual| actual.is(value.bind(py))) {
                    return integrity(format!("Runtime dependency {name} has been rebound"));
                }
            }
            Dependency::ImportedModule {
                name,
                module,
                members,
            } => {
                verify_binding(
                    &bindings,
                    name,
                    module,
                    "Dependency module",
                    "has been rebound",
                )?;
                verify_members(
                    module.bind(py).dict(),
                    name,
                    members,
                    "Dependency module",
                    "has been rebound",
                )?;
            }
            Dependency::TrustedModule {
                name,
                module,
                members,
            } => {
                verify_binding(
                    &bindings,
                    name,
                    module,
                    "Registered module",
                    "has been rebound",
                )?;
                verify_members(
                    module.bind(py).dict(),
                    name,
                    members,
                    "Registered symbol",
                    "has been replaced",
                )?;
            }
            Dependency::TrustedSymbol { name, value } => {
                verify_binding(
                    &bindings,
                    name,
                    value,
                    "Registered symbol",
                    "has been rebound",
                )?;
            }
            Dependency::ExternalSymbol {
                name,
                value,
                boundary,
            } => {
                verify_binding(
                    &bindings,
                    name,
                    value,
                    "External symbol",
                    "has been rebound",
                )?;
                verify_external_boundary(py, name, value, boundary)?;
            }
            Dependency::ExternalModule {
                name,
                module,
                members,
            } => {
                verify_binding(
                    &bindings,
                    name,
                    module,
                    "External module",
                    "has been rebound",
                )?;
                let module_bindings = module.bind(py).dict();
                for member in members {
                    if !module_bindings
                        .get_item(&member.name)?
                        .is_some_and(|actual| actual.is(member.value.bind(py)))
                    {
                        return integrity(format!(
                            "External symbol {name}.{} has been replaced",
                            member.name
                        ));
                    }
                    verify_external_boundary(
                        py,
                        &format!("{name}.{}", member.name),
                        &member.value,
                        &member.boundary,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn verify_binding<T>(
    bindings: &Bound<'_, PyDict>,
    name: &str,
    expected: &Py<T>,
    subject: &str,
    failure: &str,
) -> Result<(), RuntimeFailure> {
    if !bindings
        .get_item(name)?
        .is_some_and(|actual| actual.is(expected.bind(bindings.py())))
    {
        return integrity(format!("{subject} {name} {failure}"));
    }
    Ok(())
}

fn verify_members(
    bindings: Bound<'_, PyDict>,
    module_name: &str,
    members: &[(String, Py<PyAny>)],
    subject: &str,
    failure: &str,
) -> Result<(), RuntimeFailure> {
    for (name, expected) in members {
        if !bindings
            .get_item(name)?
            .is_some_and(|actual| actual.is(expected.bind(bindings.py())))
        {
            return integrity(format!("{subject} {module_name}.{name} {failure}"));
        }
    }
    Ok(())
}

fn verify_external_boundary(
    py: Python<'_>,
    name: &str,
    value: &Py<PyAny>,
    boundary: &ExternalBoundarySeal,
) -> Result<(), RuntimeFailure> {
    let ExternalBoundarySeal::Audited {
        public_module,
        public_member,
        code,
    } = boundary
    else {
        return Ok(());
    };
    if !public_module
        .bind(py)
        .dict()
        .get_item(public_member)?
        .is_some_and(|actual| actual.is(value.bind(py)))
    {
        return integrity(format!("Audited external symbol {name} has been replaced"));
    }
    if let Some(expected_code) = code
        && !value
            .bind(py)
            .getattr("__code__")?
            .is(expected_code.bind(py))
    {
        return integrity(format!(
            "Audited external symbol {name} has a code identity mismatch"
        ));
    }
    Ok(())
}

fn integrity<T>(message: impl Into<String>) -> Result<T, RuntimeFailure> {
    Err(RuntimeFailure::Integrity(message.into()))
}
