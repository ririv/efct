use napi_derive::napi;
use serde_json::Value;

#[napi]
pub fn protocol_version() -> u32 {
    efct_protocol::PROTOCOL_VERSION
}

#[napi]
pub fn check_envelope(value: Value) -> napi::Result<Value> {
    let envelope: efct_protocol::ProtocolEnvelope = serde_json::from_value(value)
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct envelope: {error}")))?;
    efct_protocol::validate(&envelope)
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct envelope: {error}")))?;
    serde_json::to_value(efct_core::check(envelope))
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct diagnostics: {error}")))
}

#[napi]
pub fn check_project(value: Value) -> napi::Result<Value> {
    let project: efct_protocol::ProjectEnvelope = serde_json::from_value(value)
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct project: {error}")))?;
    efct_protocol::validate_project(&project)
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct project: {error}")))?;
    serde_json::to_value(efct_core::check_project(project))
        .map_err(|error| napi::Error::from_reason(format!("Invalid Efct diagnostics: {error}")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_the_current_protocol_version() {
        assert_eq!(super::protocol_version(), 1);
    }

    #[test]
    fn rejects_an_invalid_typed_envelope() {
        let result = super::check_envelope(serde_json::json!({
            "protocol_version": 1,
            "filename": "invalid.ts",
            "source_sha256": "invalid",
            "language": {
                "kind": "typescript",
                "compiler": {
                    "version": "5.9.3",
                    "installation_sha256": "a".repeat(64)
                },
                "runtime": { "version": [24, 19, 0], "node_api_version": 8 },
                "config_sha256": "a".repeat(64),
                "root": { "items": [] }
            }
        }));

        assert!(result.is_err());
    }
}
