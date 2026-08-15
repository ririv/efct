use efct_model::ExternalEffect;

use crate::exceptions::BuiltinExceptionKind;

use super::{ApiEffect, ApiSignatureType, Operation, OperationEffects};

pub(super) static OPERATIONS: &[Operation] = &[
    Operation {
        name: "time.time_ns",
        parameters: &[],
        returns: ApiSignatureType::Int,
        effects: OperationEffects::Fixed(&[ApiEffect::External(ExternalEffect::Clock)]),
    },
    Operation {
        name: "random.randint",
        parameters: &[ApiSignatureType::Int, ApiSignatureType::Int],
        returns: ApiSignatureType::Int,
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::Random),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
        ]),
    },
    Operation {
        name: "os.urandom",
        parameters: &[ApiSignatureType::Int],
        returns: ApiSignatureType::Bytes,
        effects: OperationEffects::Fixed(&[
            ApiEffect::External(ExternalEffect::Random),
            ApiEffect::Raise(BuiltinExceptionKind::MissingImplementation),
            ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
            ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
            ApiEffect::Diverge,
        ]),
    },
];
