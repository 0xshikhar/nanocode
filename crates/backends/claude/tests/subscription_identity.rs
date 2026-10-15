#[path = "../src/subscription_identity.rs"]
#[cfg(not(target_family = "wasm"))]
mod subscription_identity;

use nanocode_backend_claude::SubscriptionIdentity;
use serde_json::{Value, json};
#[cfg(not(target_family = "wasm"))]
use subscription_identity::is_legacy_user_id;

fn legacy_id() -> String {
    format!(
        "user_{}_account_12345678-90ab-cdef-0123-456789abcdef_session_fedcba98-7654-3210-fedc-ba9876543210",
        "aB09".repeat(16)
    )
}

#[test]
fn legacy_and_json_ids_pass_through_unchanged() {
    for user_id in [
        legacy_id(),
        r#"{"session_id":"existing","custom":[1,true]}"#.into(),
    ] {
        let identity = SubscriptionIdentity {
            user_id: Some(user_id.clone()),
            ..Default::default()
        };
        assert_eq!(
            identity.metadata("new").unwrap(),
            json!({"user_id":user_id})
        );
    }
}

#[test]
fn invalid_ids_fall_back_to_stable_device_metadata() {
    for user_id in ["invalid", "{\"session_id\":\"\"}", "{\"session_id\":1}"] {
        for (account, digest) in [
            (
                None,
                "6463ada4aa5bf6f35374259f8e1b3eb4be8a275abfb57ff05d92ffeff893d005",
            ),
            (
                Some("account"),
                "ba954c55410a7cce3187c0fc769f3735ac42a2a9b9e935a98a62034615c83c40",
            ),
        ] {
            let identity = SubscriptionIdentity {
                install_id: Some("install".into()),
                account_uuid: account.map(str::to_owned),
                user_id: Some(user_id.into()),
                ..Default::default()
            };
            let metadata = identity.metadata("new").unwrap();
            let user: Value = serde_json::from_str(metadata["user_id"].as_str().unwrap()).unwrap();
            let mut expected = json!({"device_id":digest,"session_id":"new"});
            if let Some(account) = account {
                expected["account_uuid"] = json!(account);
            }
            assert_eq!(user, expected);
        }
    }
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn byte_parser_matches_the_previous_protocol_pattern() {
    let previous = regex::Regex::new(r"^user_[0-9a-fA-F]{64}_account_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}_session_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap();
    let valid = legacy_id();
    let compare = |value: &str| {
        assert_eq!(
            is_legacy_user_id(value),
            previous.is_match(value),
            "{value:?}"
        );
    };
    compare(&valid);
    compare(&valid.to_lowercase());
    compare(&valid.to_uppercase());
    for position in 0..valid.len() {
        let mut bytes = valid.as_bytes().to_vec();
        for byte in 0..=127 {
            bytes[position] = byte;
            compare(std::str::from_utf8(&bytes).unwrap());
        }
        let mut removed = valid.clone();
        removed.remove(position);
        compare(&removed);
    }
    for position in 0..=valid.len() {
        for inserted in ["a", "_", "-", "\n", "\0", "é", "０", "🦀"] {
            let mut value = valid.clone();
            value.insert_str(position, inserted);
            compare(&value);
        }
        compare(&valid[..position]);
    }
    for suffix in [
        "\n",
        "\r\n",
        "_session_00000000-0000-0000-0000-000000000000",
    ] {
        compare(&format!("{valid}{suffix}"));
    }
}
