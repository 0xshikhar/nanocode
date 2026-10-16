pub(crate) fn is_legacy_user_id(value: &str) -> bool {
    let Some(value) = value.strip_prefix("user_") else {
        return false;
    };
    let Some((device, value)) = value.split_once("_account_") else {
        return false;
    };
    if device.len() != 64 || !device.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let Some((account, session)) = value.split_once("_session_") else {
        return false;
    };
    is_lowercase_uuid(account) && is_lowercase_uuid(session)
}

fn is_lowercase_uuid(value: &str) -> bool {
    value.len() == uuid::fmt::Hyphenated::LENGTH
        && !value.bytes().any(|byte| byte.is_ascii_uppercase())
        && uuid::Uuid::parse_str(value).is_ok()
}
