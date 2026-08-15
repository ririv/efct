use efct_model::ExternalEffect;

use crate::exceptions::BuiltinExceptionKind;

use super::{ApiEffect, ApiSignatureType, Operation, OperationEffects};

pub(super) static OPERATIONS: &[Operation] = &[
    Operation {
        name: "os.system",
        parameters: &[ApiSignatureType::Str],
        returns: ApiSignatureType::Int,
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::Process),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
            ApiEffect::Diverge,
        ]),
    },
    Operation {
        name: "os.popen",
        parameters: &[ApiSignatureType::Str],
        returns: ApiSignatureType::External("os._wrap_close"),
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::Process),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
        ]),
    },
];
