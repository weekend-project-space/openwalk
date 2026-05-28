use scheme4r::{
    runtime::{ErrorObject, ErrorObjectKind},
    SchemeError, Value,
};

pub(super) fn scheme_host_error(err: anyhow::Error) -> SchemeError {
    let message = format!("{err:#}");
    let object = Value::error_object(ErrorObject::new(
        ErrorObjectKind::General,
        message,
        Vec::new(),
    ));
    SchemeError::raised(object, true)
}

pub(super) fn scheme_error_to_anyhow(
    context: impl Into<String>,
    err: SchemeError,
) -> anyhow::Error {
    let context = context.into();
    if let Some((object, _continuable)) = err.as_raised() {
        return anyhow::anyhow!("{context}: {}", scheme_exception_message(object));
    }
    anyhow::anyhow!("{context}: {err}")
}

fn scheme_exception_message(value: &Value) -> String {
    match value {
        Value::ErrorObject(error) => error.message().to_string(),
        other => other.to_string(),
    }
}
