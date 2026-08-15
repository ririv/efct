use efct_model::ExternalEffect;

use crate::exceptions::BuiltinExceptionKind;

use super::{ApiEffect, ApiSignatureType, Operation, OperationEffects};

pub(super) static OPERATIONS: &[Operation] = &[
    Operation {
        name: "os.getenv",
        parameters: &[ApiSignatureType::Str, ApiSignatureType::Str],
        returns: ApiSignatureType::Str,
        effects: OperationEffects::Fixed(&[ApiEffect::External(ExternalEffect::Environment)]),
    },
    Operation {
        name: "os.putenv",
        parameters: &[ApiSignatureType::Str, ApiSignatureType::Str],
        returns: ApiSignatureType::None,
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::Environment),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
        ]),
    },
];
