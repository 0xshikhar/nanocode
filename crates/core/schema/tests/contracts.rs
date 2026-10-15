use nanocode_schema::validator_for;
use serde_json::{Value, json};

#[test]
fn invalid_schemas_and_unavailable_references_are_rejected() {
    for schema in [
        json!(42),
        json!({"type": "invalid"}),
        json!({"required": "name"}),
        json!({"multipleOf": 0}),
        json!({"properties": {"name": 7}}),
        json!({"pattern": "["}),
        json!({"$ref": "https://unavailable.invalid/schema"}),
        json!({"$ref": "file:///unavailable-schema.json"}),
    ] {
        assert!(validator_for(&schema).is_err(), "{schema}");
    }
}

#[test]
fn local_references_and_escaped_paths_are_preserved() {
    let validator = validator_for(&json!({
        "$defs": {"entry": {"type": "integer"}},
        "type": "object",
        "properties": {"a/b~c": {"$ref": "#/$defs/entry"}}
    }))
    .unwrap();
    assert!(validator.is_valid(&json!({"a/b~c": 3})));
    let errors = validator
        .iter_errors(&json!({"a/b~c": "bad"}))
        .collect::<Vec<_>>();
    assert!(errors.iter().any(|e| e.instance_path() == "/a~1b~0c"));
    assert!(errors.iter().any(|e| e.schema_path().contains("/type")));
}

#[test]
fn drafts_preserve_integer_and_content_rules() {
    let legacy = validator_for(&json!({
        "$schema": "http://json-schema.org/draft-04/schema#", "type": "integer"
    }))
    .unwrap();
    assert!(legacy.is_valid(&json!(1)));
    assert!(!legacy.is_valid(&json!(1.0)));
    for draft in [
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema",
        "https://json-schema.org/draft/2020-12/schema",
    ] {
        assert!(
            validator_for(&json!({"$schema":draft,"type":"integer"}))
                .unwrap()
                .is_valid(&json!(1.0))
        );
    }
    let schema =
        json!({"$schema":"http://json-schema.org/draft-07/schema#", "contentEncoding":"base64"});
    assert!(!validator_for(&schema).unwrap().is_valid(&json!("%%%")));
    assert!(
        validator_for(&json!({"contentEncoding":"base64"}))
            .unwrap()
            .is_valid(&json!("%%%"))
    );
}

#[test]
fn formats_follow_draft_defaults() {
    assert!(
        validator_for(&json!({"format":"email"}))
            .unwrap()
            .is_valid(&json!("bad"))
    );
    for (format, valid, invalid) in [
        ("email", "\"a\\\"b\"@example.com", "@example.com"),
        ("hostname", "example.com", "example.com."),
        ("uri-template", "/users/{name}", "/users/{}"),
        ("iri", "https://example.com/世界", "https://example.com/%ZZ"),
    ] {
        let validator = validator_for(
            &json!({"$schema":"http://json-schema.org/draft-07/schema#","format":format}),
        )
        .unwrap();
        assert!(validator.is_valid(&json!(valid)), "{format}");
        assert!(!validator.is_valid(&json!(invalid)), "{format}");
    }
}

#[test]
fn patterns_support_ecmascript_lookaround_backreferences_and_unicode() {
    for (pattern, valid, invalid) in [
        (r"^(?=a)a$", "a", "b"),
        (r"^(a)\1$", "aa", "ab"),
        (r"^\d$", "3", "٣"),
        (r"^.$", "😀", "ab"),
    ] {
        let validator = validator_for(&json!({"pattern":pattern})).unwrap();
        assert!(validator.is_valid(&json!(valid)), "{pattern}");
        assert!(!validator.is_valid(&json!(invalid)), "{pattern}");
    }
}

#[test]
fn numeric_limits_keep_integer_precision_and_decimal_multiples() {
    let validator = validator_for(&json!({"minimum":9007199254740993u64})).unwrap();
    assert!(!validator.is_valid(&json!(9007199254740992u64)));
    assert!(!validator.is_valid(&json!(9007199254740992.0)));
    assert!(validator.is_valid(&json!(9007199254740993u64)));
    for (divisor, valid, invalid) in [
        (json!(0.1), json!(0.3), json!(0.31)),
        (json!(0.5), json!(1e308), json!(0.25)),
        (
            json!(3),
            json!(18446744073709551615u64),
            json!(18446744073709551614u64),
        ),
    ] {
        let validator = validator_for(&json!({"multipleOf":divisor})).unwrap();
        assert!(validator.is_valid(&valid));
        assert!(!validator.is_valid(&invalid));
    }
    let mut values = (1..25).map(Value::from).collect::<Vec<_>>();
    values.extend([json!(0.0), json!(-0.0)]);
    assert!(
        !validator_for(&json!({"uniqueItems":true}))
            .unwrap()
            .is_valid(&json!(values))
    );
}

#[test]
fn modern_evaluation_and_dynamic_references_work() {
    let validator = validator_for(&json!({
        "$id":"https://example.com/tree", "$dynamicAnchor":"node", "type":"object",
        "properties":{"children":{"type":"array","items":{"$dynamicRef":"#node"}}},
        "unevaluatedProperties":false
    }))
    .unwrap();
    assert!(validator.is_valid(&json!({"children":[{"children":[]}]})));
    assert!(!validator.is_valid(&json!({"children":[{"extra":true}]})));
}

#[test]
fn compiled_contracts_are_send_sync_and_leave_serialization_unchanged() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<nanocode_schema::Validator>();
    assert_eq!(
        serde_json::to_string(&json!({"z":1,"a":2})).unwrap(),
        r#"{"a":2,"z":1}"#
    );
    assert!(validator_for(&json!(true)).unwrap().is_valid(&Value::Null));
    assert!(!validator_for(&json!(false)).unwrap().is_valid(&Value::Null));
}

#[test]
fn large_count_limits_preserve_native_and_wasm_semantics() {
    for limit in [json!(4294967296u64), json!(1e100)] {
        for (minimum, maximum, value) in [
            ("minLength", "maxLength", json!("x")),
            ("minItems", "maxItems", json!([1])),
            ("minProperties", "maxProperties", json!({"x":1})),
            ("minContains", "maxContains", json!([1])),
        ] {
            let mut schema = json!({"contains":true});
            schema[minimum] = limit.clone();
            assert!(
                !validator_for(&schema).unwrap().is_valid(&value),
                "{schema}"
            );
            let mut schema = json!({"contains":true});
            schema[maximum] = limit.clone();
            assert!(validator_for(&schema).unwrap().is_valid(&value), "{schema}");
        }
    }
}
