use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule, PyType};
use sha2::{Digest, Sha256};

use crate::certificate::{AuditedImplementation, BoundaryEvidence, Certificate};
use crate::contract::{ValueTypes, matches_type};
use crate::error::RuntimeFailure;
use crate::function::with_verified_certificate;
use crate::python_code::{PythonCodeSourceMatch, validate_python_code_source};

pub struct Environment {
    builtins: Py<PyModule>,
    builtin_members: Vec<(String, Py<PyAny>)>,
    trusted_modules: Vec<TrustedModule>,
    function_type: Py<PyAny>,
    namespace_type: Py<PyAny>,
    pure_record_fields: Py<PyAny>,
    builtin_function_type: Py<PyAny>,
}

struct TrustedModule {
    name: String,
    module: Py<PyModule>,
    members: Vec<(String, Py<PyAny>)>,
}

pub struct SealedExecution {
    pub function: Py<PyAny>,
    pub dependencies: Vec<Dependency>,
}

pub enum Dependency {
    Module {
        name: String,
        value: Py<PyAny>,
    },
    Builtin {
        name: String,
        value: Py<PyAny>,
    },
    ImportedModule {
        name: String,
        module: Py<PyModule>,
        members: Vec<(String, Py<PyAny>)>,
    },
    TrustedModule {
        name: String,
        module: Py<PyModule>,
        members: Vec<(String, Py<PyAny>)>,
    },
    TrustedSymbol {
        name: String,
        value: Py<PyAny>,
    },
    ExternalSymbol {
        name: String,
        value: Py<PyAny>,
        boundary: ExternalBoundarySeal,
    },
    ExternalModule {
        name: String,
        module: Py<PyModule>,
        members: Vec<ExternalDependencyMember>,
    },
}

pub struct ExternalDependencyMember {
    pub(crate) name: String,
    pub(crate) value: Py<PyAny>,
    pub(crate) boundary: ExternalBoundarySeal,
}

pub enum ExternalBoundarySeal {
    Audited {
        public_module: Py<PyModule>,
        public_member: String,
        code: Option<Py<PyAny>>,
    },
    Unsafe,
}

impl Environment {
    pub fn load(py: Python<'_>) -> PyResult<Self> {
        let builtins = py.import("builtins")?;
        let builtin_names = efct_language_python::registered_builtin_exception_names()
            .chain(["len", "open", "print", "range", "str", "sum", "frozenset"])
            .collect::<BTreeSet<_>>();
        let builtin_members = builtin_names
            .into_iter()
            .map(|name| Ok((name.to_owned(), builtins.getattr(name)?.unbind())))
            .collect::<PyResult<_>>()?;
        let mut registered = BTreeMap::<&str, BTreeSet<&str>>::new();
        for (module, member) in efct_language_python::registered_api_members() {
            registered.entry(module).or_default().insert(member);
        }
        let trusted_modules = registered
            .into_iter()
            .map(|(name, member_names)| {
                let module = py.import(name)?;
                let members = member_names
                    .into_iter()
                    .map(|member| Ok((member.to_owned(), module.getattr(member)?.unbind())))
                    .collect::<PyResult<_>>()?;
                Ok(TrustedModule {
                    name: name.to_owned(),
                    module: module.unbind(),
                    members,
                })
            })
            .collect::<PyResult<_>>()?;
        let types = py.import("types")?;
        Ok(Self {
            builtins: builtins.unbind(),
            builtin_members,
            trusted_modules,
            function_type: types.getattr("FunctionType")?.unbind(),
            namespace_type: types.getattr("SimpleNamespace")?.unbind(),
            pure_record_fields: py
                .import("efct.values")?
                .getattr("_pure_record_fields")?
                .unbind(),
            builtin_function_type: types.getattr("BuiltinFunctionType")?.unbind(),
        })
    }

    fn builtin(&self, name: &str) -> Option<&Py<PyAny>> {
        self.builtin_members
            .iter()
            .find_map(|(member, value)| (member == name).then_some(value))
    }

    fn trusted_module(&self, name: &str) -> Option<&TrustedModule> {
        self.trusted_modules
            .iter()
            .find(|module| module.name == name)
    }

    fn trusted_member(&self, module: &str, member: &str) -> Option<&Py<PyAny>> {
        self.trusted_module(module)?
            .members
            .iter()
            .find_map(|(registered_name, value)| (registered_name == member).then_some(value))
    }

    pub(crate) fn builtins(&self) -> &Py<PyModule> {
        &self.builtins
    }
}

impl SealedExecution {
    pub fn clone_ref(&self, py: Python<'_>) -> Self {
        Self {
            function: self.function.clone_ref(py),
            dependencies: self
                .dependencies
                .iter()
                .map(|dependency| dependency.clone_ref(py))
                .collect(),
        }
    }
}

impl Dependency {
    fn clone_ref(&self, py: Python<'_>) -> Self {
        match self {
            Self::Module { name, value } => Self::Module {
                name: name.clone(),
                value: value.clone_ref(py),
            },
            Self::Builtin { name, value } => Self::Builtin {
                name: name.clone(),
                value: value.clone_ref(py),
            },
            Self::ImportedModule {
                name,
                module,
                members,
            } => Self::ImportedModule {
                name: name.clone(),
                module: module.clone_ref(py),
                members: clone_members(members, py),
            },
            Self::TrustedModule {
                name,
                module,
                members,
            } => Self::TrustedModule {
                name: name.clone(),
                module: module.clone_ref(py),
                members: clone_members(members, py),
            },
            Self::TrustedSymbol { name, value } => Self::TrustedSymbol {
                name: name.clone(),
                value: value.clone_ref(py),
            },
            Self::ExternalSymbol {
                name,
                value,
                boundary,
            } => Self::ExternalSymbol {
                name: name.clone(),
                value: value.clone_ref(py),
                boundary: boundary.clone_ref(py),
            },
            Self::ExternalModule {
                name,
                module,
                members,
            } => Self::ExternalModule {
                name: name.clone(),
                module: module.clone_ref(py),
                members: members
                    .iter()
                    .map(|member| ExternalDependencyMember {
                        name: member.name.clone(),
                        value: member.value.clone_ref(py),
                        boundary: member.boundary.clone_ref(py),
                    })
                    .collect(),
            },
        }
    }
}

impl ExternalBoundarySeal {
    fn clone_ref(&self, py: Python<'_>) -> Self {
        match self {
            Self::Audited {
                public_module,
                public_member,
                code,
            } => Self::Audited {
                public_module: public_module.clone_ref(py),
                public_member: public_member.clone(),
                code: code.as_ref().map(|value| value.clone_ref(py)),
            },
            Self::Unsafe => Self::Unsafe,
        }
    }
}

pub fn seal(
    py: Python<'_>,
    code: &Py<PyAny>,
    module: &Py<PyModule>,
    certificate: &Certificate,
    environment: &Environment,
    value_types: &ValueTypes,
) -> Result<SealedExecution, RuntimeFailure> {
    let private_globals = PyDict::new(py);
    private_globals.set_item("__builtins__", PyDict::new(py))?;
    let bindings = module.bind(py).dict();
    let mut dependencies = Vec::new();
    for name in &certificate.dependency_names {
        let Some(value) = bindings.get_item(name)? else {
            let Some(value) = environment.builtin(name) else {
                return integrity(format!("Runtime dependency {name} cannot be bound"));
            };
            private_globals.set_item(name, value.bind(py))?;
            dependencies.push(Dependency::Builtin {
                name: name.clone(),
                value: value.clone_ref(py),
            });
            continue;
        };
        if let Some(external) = certificate
            .external_functions
            .iter()
            .find(|external| external.binding == *name)
        {
            if !value.is_callable() {
                return integrity(format!("External symbol {name} is not callable"));
            }
            let boundary = seal_external_boundary(
                py,
                &value,
                &external.module,
                &external.name,
                &external.boundary,
                environment,
            )?;
            private_globals.set_item(name, &value)?;
            dependencies.push(Dependency::ExternalSymbol {
                name: name.clone(),
                value: value.unbind(),
                boundary,
            });
            continue;
        }
        if let Some(external) = certificate
            .external_modules
            .iter()
            .find(|external| external.binding == *name)
        {
            let Ok(module_value) = value.cast::<PyModule>() else {
                return integrity(format!("External module {name} has an identity mismatch"));
            };
            let module_name = module_value.name()?.to_str()?.to_owned();
            if module_name != external.module {
                return integrity(format!("External module {name} has an identity mismatch"));
            }
            let mut members = Vec::new();
            for member in &external.members {
                let Some(member_value) = module_value.dict().get_item(&member.name)? else {
                    return integrity(format!(
                        "External symbol {name}.{} is not callable",
                        member.name
                    ));
                };
                if !member_value.is_callable() {
                    return integrity(format!(
                        "External symbol {name}.{} is not callable",
                        member.name
                    ));
                }
                let boundary = seal_external_boundary(
                    py,
                    &member_value,
                    &external.module,
                    &member.name,
                    &member.boundary,
                    environment,
                )?;
                members.push(ExternalDependencyMember {
                    name: member.name.clone(),
                    value: member_value.unbind(),
                    boundary,
                });
            }
            private_globals.set_item(
                name,
                namespace(
                    py,
                    environment,
                    members.iter().map(|member| (&member.name, &member.value)),
                )?,
            )?;
            dependencies.push(Dependency::ExternalModule {
                name: name.clone(),
                module: module_value.clone().unbind(),
                members,
            });
            continue;
        }
        if let Some((_, imported_module, imported_name)) = certificate
            .imported_functions
            .iter()
            .find(|(binding, _, _)| binding == name)
            && let Some(registered) = environment.trusted_member(imported_module, imported_name)
        {
            if !value.is(registered.bind(py)) {
                return integrity(format!("Registered symbol {name} has an identity mismatch"));
            }
            private_globals.set_item(name, registered.bind(py))?;
            dependencies.push(Dependency::TrustedSymbol {
                name: name.clone(),
                value: registered.clone_ref(py),
            });
            continue;
        }
        if let Some(exception) = certificate
            .exception_bindings
            .iter()
            .find(|exception| exception.binding == *name)
        {
            if !matches_exception_type(py, &value, exception, environment)? {
                return integrity(format!(
                    "Registered exception type {name} has an identity mismatch"
                ));
            }
            private_globals.set_item(name, &value)?;
            dependencies.push(Dependency::Module {
                name: name.clone(),
                value: value.unbind(),
            });
            continue;
        }
        if with_verified_certificate(&value, |dependency| {
            verify_function_certificate(certificate, name, dependency)
        })
        .transpose()?
        .is_some()
        {
            private_globals.set_item(name, &value)?;
            dependencies.push(Dependency::Module {
                name: name.clone(),
                value: value.unbind(),
            });
            continue;
        }
        if let Ok(module_value) = value.cast::<PyModule>() {
            if module_value.name()? == "efct" {
                let members = value_types.pure_value_members(py);
                for (member_name, expected) in &members {
                    if !module_value
                        .dict()
                        .get_item(member_name)?
                        .is_some_and(|actual| actual.is(expected.bind(py)))
                    {
                        return integrity("An efct pure-value constructor has been replaced");
                    }
                }
                private_globals.set_item(
                    name,
                    namespace(
                        py,
                        environment,
                        members.iter().map(|(name, value)| (name, value)),
                    )?,
                )?;
                dependencies.push(Dependency::TrustedModule {
                    name: name.clone(),
                    module: module_value.clone().unbind(),
                    members,
                });
                continue;
            }
            let module_name = module_value.name()?.to_str()?.to_owned();
            if let Some(trusted) = environment.trusted_module(&module_name) {
                if !module_value.is(trusted.module.bind(py)) {
                    return integrity(format!("Registered module {name} has an identity mismatch"));
                }
                let Some((_, _, expected_members)) =
                    certificate
                        .imported_modules
                        .iter()
                        .find(|(binding, imported_module, _)| {
                            binding == name && imported_module == &module_name
                        })
                else {
                    return integrity(format!(
                        "Registered module {name} does not match its static import binding"
                    ));
                };
                let members = expected_members
                    .iter()
                    .map(|expected| {
                        trusted
                            .members
                            .iter()
                            .find(|(registered, _)| registered == expected)
                            .map(|(member, value)| (member.clone(), value.clone_ref(py)))
                            .ok_or_else(|| {
                                RuntimeFailure::Integrity(format!(
                                    "Registered symbol {name}.{expected} is not available"
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                private_globals.set_item(
                    name,
                    namespace(
                        py,
                        environment,
                        members.iter().map(|(member, value)| (member, value)),
                    )?,
                )?;
                dependencies.push(Dependency::TrustedModule {
                    name: name.clone(),
                    module: trusted.module.clone_ref(py),
                    members,
                });
                continue;
            }
            let Some(expected_hash) = certificate.dependency_source(&module_name) else {
                return integrity(format!(
                    "Dependency module {name} is not included in the project certificate"
                ));
            };
            let Some((_, _, expected_members)) =
                certificate
                    .imported_modules
                    .iter()
                    .find(|(binding, imported_module, _)| {
                        binding == name && imported_module == &module_name
                    })
            else {
                return integrity(format!(
                    "Dependency module {name} does not match its static import binding"
                ));
            };
            let mut members = Vec::new();
            for member_name in expected_members {
                let Some(member) = module_value.dict().get_item(member_name)? else {
                    return integrity(format!(
                        "Dependency symbol {name}.{member_name} is not a verified function"
                    ));
                };
                let exception_binding = format!("{name}.{member_name}");
                if let Some(exception) = certificate
                    .exception_bindings
                    .iter()
                    .find(|exception| exception.binding == exception_binding)
                {
                    if !matches_exception_type(py, &member, exception, environment)? {
                        return integrity(format!(
                            "Registered exception type {exception_binding} has an identity mismatch"
                        ));
                    }
                } else {
                    let Some(matches) = with_verified_certificate(&member, |member_certificate| {
                        member_certificate.module_name == module_name
                            && member_certificate.function_name == *member_name
                            && member_certificate.source_sha256 == expected_hash
                    }) else {
                        return integrity(format!(
                            "Dependency symbol {name}.{member_name} is not a verified function"
                        ));
                    };
                    if !matches {
                        return integrity(format!(
                            "Dependency symbol {name}.{member_name} has a certificate mismatch"
                        ));
                    }
                }
                members.push((member_name.clone(), member.unbind()));
            }
            private_globals.set_item(
                name,
                namespace(
                    py,
                    environment,
                    members.iter().map(|(name, value)| (name, value)),
                )?,
            )?;
            dependencies.push(Dependency::ImportedModule {
                name: name.clone(),
                module: module_value.clone().unbind(),
                members,
            });
            continue;
        }
        let is_record = value.get_type().is(py.get_type::<PyType>())
            && !environment
                .pure_record_fields
                .bind(py)
                .call1((&value,))?
                .is_none();
        if is_record {
            private_globals.set_item(name, &value)?;
            dependencies.push(Dependency::Module {
                name: name.clone(),
                value: value.unbind(),
            });
            continue;
        }
        let Some(expected) = certificate.constant_type(name) else {
            return integrity(format!(
                "Global name {name} is not a type-compatible verified pure constant"
            ));
        };
        if !matches_type(py, &value, expected, value_types, &mut Default::default())? {
            return integrity(format!(
                "Global name {name} is not a type-compatible verified pure constant"
            ));
        }
        private_globals.set_item(name, &value)?;
        dependencies.push(Dependency::Module {
            name: name.clone(),
            value: value.unbind(),
        });
    }
    let function = environment.function_type.bind(py).call1((
        code.bind(py),
        private_globals,
        certificate.function_name.as_str(),
    ))?;
    Ok(SealedExecution {
        function: function.unbind(),
        dependencies,
    })
}

fn matches_exception_type(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
    expected: &efct_language_python::ExceptionBinding,
    environment: &Environment,
) -> Result<bool, RuntimeFailure> {
    let Ok(exception_type) = value.cast::<PyType>() else {
        return Ok(false);
    };
    let Some(exception) = environment.builtin("Exception") else {
        return integrity("The registered builtins.Exception type is unavailable");
    };
    let exception = exception
        .bind(py)
        .cast::<PyType>()
        .map_err(|error| RuntimeFailure::Python(error.into()))?;
    Ok(exception_type.is_subclass(exception)?
        && exception_type.getattr("__module__")?.extract::<String>()? == expected.module
        && exception_type
            .getattr("__qualname__")?
            .extract::<String>()?
            == expected.name)
}

fn seal_external_boundary(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
    public_module_name: &str,
    public_member_name: &str,
    evidence: &BoundaryEvidence,
    environment: &Environment,
) -> Result<ExternalBoundarySeal, RuntimeFailure> {
    let BoundaryEvidence::Audited(boundary) = evidence else {
        return Ok(ExternalBoundarySeal::Unsafe);
    };
    if boundary.path != format!("{public_module_name}.{public_member_name}")
        || boundary.public_module != public_module_name
    {
        return integrity(format!(
            "Audited symbol {} does not match its static import binding",
            boundary.path
        ));
    }
    let public_module = loaded_module(py, public_module_name)?;
    if !public_module
        .dict()
        .get_item(public_member_name)?
        .is_some_and(|actual| actual.is(value))
    {
        return integrity(format!(
            "Audited symbol {} does not match its public module binding",
            boundary.path
        ));
    }
    verify_module_origin(
        &public_module,
        &boundary.public_artifact_path,
        &boundary.path,
    )?;
    let code = match &boundary.implementation {
        AuditedImplementation::Python {
            module,
            qualname,
            artifact_path,
            artifact_sha256,
        } => {
            if !value.get_type().is(environment.function_type.bind(py)) {
                return integrity(format!(
                    "Audited symbol {} is not a Python function",
                    boundary.path
                ));
            }
            verify_callable_name(value, module, qualname, &boundary.path)?;
            let implementation_module = loaded_module(py, module)?;
            verify_module_origin(&implementation_module, artifact_path, &boundary.path)?;
            verify_implementation_binding(value, &implementation_module, qualname, &boundary.path)?;
            if !value
                .getattr("__globals__")?
                .is(implementation_module.dict())
            {
                return integrity(format!(
                    "Audited symbol {} has an implementation globals mismatch",
                    boundary.path
                ));
            }
            let code = value.getattr("__code__")?;
            let code_path = code.getattr("co_filename")?.extract::<String>()?;
            if canonical_path(&code_path)? != canonical_path(artifact_path)? {
                return integrity(format!(
                    "Audited symbol {} has a code origin mismatch",
                    boundary.path
                ));
            }
            verify_python_source(
                py,
                &code,
                qualname,
                artifact_path,
                artifact_sha256,
                &boundary.path,
            )?;
            Some(code.unbind())
        }
        AuditedImplementation::Native {
            module,
            qualname,
            artifact_path,
        } => {
            if !value
                .get_type()
                .is(environment.builtin_function_type.bind(py))
            {
                return integrity(format!(
                    "Audited symbol {} is not a native function",
                    boundary.path
                ));
            }
            verify_callable_name(value, module, qualname, &boundary.path)?;
            let implementation_module = loaded_module(py, module)?;
            verify_module_origin(&implementation_module, artifact_path, &boundary.path)?;
            verify_implementation_binding(value, &implementation_module, qualname, &boundary.path)?;
            None
        }
    };
    Ok(ExternalBoundarySeal::Audited {
        public_module: public_module.unbind(),
        public_member: public_member_name.to_owned(),
        code,
    })
}

fn verify_python_source(
    py: Python<'_>,
    code: &Bound<'_, PyAny>,
    qualname: &str,
    artifact_path: &str,
    expected_sha256: &str,
    symbol: &str,
) -> Result<(), RuntimeFailure> {
    let raw = fs::read(artifact_path).map_err(|error| {
        RuntimeFailure::Integrity(format!(
            "Cannot read audited source {artifact_path}: {error}"
        ))
    })?;
    if format!("{:x}", Sha256::digest(&raw)) != expected_sha256 {
        return integrity(format!(
            "Audited symbol {symbol} has an installation source digest mismatch"
        ));
    }
    match validate_python_code_source(py, code, qualname, &raw)? {
        PythonCodeSourceMatch::Match { .. } => Ok(()),
        PythonCodeSourceMatch::NotFound => integrity(format!(
            "Audited symbol {symbol} cannot be uniquely located in its installation source"
        )),
        PythonCodeSourceMatch::Mismatch => integrity(format!(
            "Audited symbol {symbol} code does not match its installation source"
        )),
    }
}

fn loaded_module<'py>(py: Python<'py>, name: &str) -> Result<Bound<'py, PyModule>, RuntimeFailure> {
    let modules = py
        .import("sys")?
        .getattr("modules")?
        .cast_into::<PyDict>()
        .map_err(|error| RuntimeFailure::Python(error.into()))?;
    let Some(module) = modules.get_item(name)? else {
        return integrity(format!("Audited module {name} is not loaded"));
    };
    module.cast_into::<PyModule>().map_err(|_| {
        RuntimeFailure::Integrity(format!("Audited module {name} has an identity mismatch"))
    })
}

fn verify_module_origin(
    module: &Bound<'_, PyModule>,
    expected: &str,
    symbol: &str,
) -> Result<(), RuntimeFailure> {
    let spec = module.getattr("__spec__")?;
    if spec.is_none() {
        return integrity(format!(
            "Audited symbol {symbol} has no module specification"
        ));
    }
    let origin = spec.getattr("origin")?;
    if origin.is_none() {
        return integrity(format!("Audited symbol {symbol} has no module origin"));
    }
    let origin = origin.extract::<String>()?;
    if canonical_path(&origin)? != canonical_path(expected)? {
        return integrity(format!(
            "Audited symbol {symbol} has a module origin mismatch"
        ));
    }
    if let Ok(filename) = module.getattr("__file__")
        && !filename.is_none()
        && canonical_path(&filename.extract::<String>()?)? != canonical_path(expected)?
    {
        return integrity(format!(
            "Audited symbol {symbol} has inconsistent module file metadata"
        ));
    }
    Ok(())
}

fn verify_callable_name(
    value: &Bound<'_, PyAny>,
    module: &str,
    qualname: &str,
    symbol: &str,
) -> Result<(), RuntimeFailure> {
    if value.getattr("__module__")?.extract::<String>()? != module
        || value.getattr("__qualname__")?.extract::<String>()? != qualname
    {
        return integrity(format!(
            "Audited symbol {symbol} has an implementation identity mismatch"
        ));
    }
    Ok(())
}

fn verify_implementation_binding(
    value: &Bound<'_, PyAny>,
    module: &Bound<'_, PyModule>,
    qualname: &str,
    symbol: &str,
) -> Result<(), RuntimeFailure> {
    let mut resolved = module.clone().into_any();
    for segment in qualname.split('.') {
        resolved = resolved.getattr(segment)?.into_any();
    }
    if !resolved.is(value) {
        return integrity(format!(
            "Audited symbol {symbol} does not match its implementation binding"
        ));
    }
    Ok(())
}

fn canonical_path(value: &str) -> Result<std::path::PathBuf, RuntimeFailure> {
    fs::canonicalize(Path::new(value)).map_err(|error| {
        RuntimeFailure::Integrity(format!("Cannot resolve audited artifact {value}: {error}"))
    })
}

fn verify_function_certificate(
    owner: &Certificate,
    name: &str,
    dependency: &Certificate,
) -> Result<(), RuntimeFailure> {
    if dependency.module_name != owner.module_name {
        if owner.dependency_source(&dependency.module_name) != Some(&dependency.source_sha256) {
            return integrity(format!(
                "Dependency function {name} has a mismatched cross-module certificate"
            ));
        }
        let expected = owner
            .imported_functions
            .iter()
            .find_map(|(binding, module, function)| {
                (binding == name).then_some((module.as_str(), function.as_str()))
            });
        if expected
            != Some((
                dependency.module_name.as_str(),
                dependency.function_name.as_str(),
            ))
        {
            return integrity(format!(
                "Dependency function {name} does not match its static import binding"
            ));
        }
    } else if dependency.source_sha256 != owner.source_sha256 {
        return integrity(format!(
            "Dependency function {name} does not match the current module certificate"
        ));
    } else if dependency.function_name != name {
        return integrity(format!(
            "Dependency function {name} has an identity mismatch"
        ));
    }
    Ok(())
}

fn namespace<'py, 'a>(
    py: Python<'py>,
    environment: &Environment,
    members: impl Iterator<Item = (&'a String, &'a Py<PyAny>)>,
) -> PyResult<Bound<'py, PyAny>> {
    let keywords = PyDict::new(py);
    for (name, value) in members {
        keywords.set_item(name, value.bind(py))?;
    }
    environment
        .namespace_type
        .bind(py)
        .call((), Some(&keywords))
}

fn clone_members(values: &[(String, Py<PyAny>)], py: Python<'_>) -> Vec<(String, Py<PyAny>)> {
    values
        .iter()
        .map(|(name, value)| (name.clone(), value.clone_ref(py)))
        .collect()
}

fn integrity<T>(message: impl Into<String>) -> Result<T, RuntimeFailure> {
    Err(RuntimeFailure::Integrity(message.into()))
}
