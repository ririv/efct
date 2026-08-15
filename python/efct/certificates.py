from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import TypeAlias


class ScalarKind(Enum):
    NONE = "None"
    BOOL = "bool"
    INT = "int"
    STR = "str"
    BYTES = "bytes"


@dataclass(frozen=True, slots=True)
class ScalarType:
    kind: ScalarKind


@dataclass(frozen=True, slots=True)
class TupleFixedType:
    elements: tuple[RuntimeType, ...]


@dataclass(frozen=True, slots=True)
class TupleVariadicType:
    element: RuntimeType


@dataclass(frozen=True, slots=True)
class FrozenSetType:
    element: RuntimeType


@dataclass(frozen=True, slots=True)
class FrozenMapType:
    key: RuntimeType
    value: RuntimeType


@dataclass(frozen=True, slots=True)
class OptionalType:
    element: RuntimeType


@dataclass(frozen=True, slots=True)
class ResultType:
    value: RuntimeType
    error: RuntimeType


@dataclass(frozen=True, slots=True)
class RecordType:
    record: type[object]
    fields: tuple[tuple[str, RuntimeType], ...]


@dataclass(frozen=True, slots=True)
class PureCallableType:
    parameters: tuple[RuntimeType, ...]
    returns: RuntimeType


@dataclass(frozen=True, slots=True)
class EffectCallableType:
    parameters: tuple[RuntimeType, ...]
    returns: RuntimeType
    effect_variable: str


RuntimeType: TypeAlias = (
    ScalarType
    | TupleFixedType
    | TupleVariadicType
    | FrozenSetType
    | FrozenMapType
    | OptionalType
    | ResultType
    | RecordType
    | PureCallableType
    | EffectCallableType
)


class CallableKind(Enum):
    INFERRED_PURE = "inferred_pure"
    BOUNDED_PURE = "bounded_pure"
    INFERRED_EFFECT = "inferred_effect"
    BOUNDED_EFFECT = "bounded_effect"


@dataclass(frozen=True, slots=True)
class AuditedBoundary:
    path: str
    owner: str
    boundary_id: str


@dataclass(frozen=True, slots=True)
class UnsafeBoundary:
    path: str
    reason: str


ExternalBoundary: TypeAlias = AuditedBoundary | UnsafeBoundary


@dataclass(frozen=True, slots=True)
class ExternalFunctionBinding:
    binding: str
    module: str
    name: str
    boundary: ExternalBoundary


@dataclass(frozen=True, slots=True)
class ExternalModuleMemberBinding:
    name: str
    boundary: ExternalBoundary


@dataclass(frozen=True, slots=True)
class ExternalModuleBinding:
    binding: str
    module: str
    members: tuple[ExternalModuleMemberBinding, ...]


@dataclass(frozen=True, slots=True)
class VerificationCertificate:
    module_name: str
    function_name: str
    callable_kind: CallableKind
    declared_effects: tuple[str, ...]
    parameter_names: tuple[str, ...]
    parameter_types: tuple[RuntimeType, ...]
    return_type: RuntimeType
    dependency_names: tuple[str, ...]
    constant_types: tuple[tuple[str, RuntimeType], ...]
    source_sha256: str
    dependency_sources: tuple[tuple[str, str], ...]
    imported_functions: tuple[tuple[str, str, str], ...]
    imported_modules: tuple[tuple[str, str, tuple[str, ...]], ...]
    external_functions: tuple[ExternalFunctionBinding, ...]
    external_modules: tuple[ExternalModuleBinding, ...]
    code_fingerprint: str
    python_version: tuple[int, int, int]
    protocol_version: int
    core_version: str
    registry_version: int
