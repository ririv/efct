use efct_model::ExternalEffect;

use crate::exceptions::BuiltinExceptionKind;

use super::{ApiEffect, ApiSignatureType, Operation, OperationEffects};

pub(super) static OPERATIONS: &[Operation] = &[
    Operation {
        name: "io.open",
        parameters: &[ApiSignatureType::Str],
        returns: ApiSignatureType::External("_io.TextIOWrapper"),
        effects: OperationEffects::FileOpenMode { parameter: 1 },
    },
    Operation {
        name: "io.open",
        parameters: &[ApiSignatureType::Str, ApiSignatureType::Str],
        returns: ApiSignatureType::External("_io.TextIOWrapper"),
        effects: OperationEffects::FileOpenMode { parameter: 1 },
    },
    Operation {
        name: "os.listdir",
        parameters: &[ApiSignatureType::Str],
        returns: ApiSignatureType::External("builtins.list[str]"),
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::FileRead),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
        ]),
    },
    Operation {
        name: "os.remove",
        parameters: &[ApiSignatureType::Str],
        returns: ApiSignatureType::None,
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::FileWrite),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
        ]),
    },
];
