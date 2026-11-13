//! Compatibility checks for the maintained YAML backend behind `serde_yaml`.

#[test]
fn yaml_backend_preserves_deployment_scalars_and_windows_newlines() {
    let text = "schemaVersion: '1.0'\nparameters:\n  enabled: true\n  literal: 'false'\n  code: '001'\n  absent: null\n  names: [alpha, beta]\n  description: |\n    Données synthétiques\n    Second line\n";
    let expected = serde_json::json!({
        "schemaVersion": "1.0",
        "parameters": {
            "enabled": true, "literal": "false", "code": "001", "absent": null,
            "names": ["alpha", "beta"],
            "description": "Données synthétiques\nSecond line\n"
        }
    });
    for input in [text.to_owned(), text.replace('\n', "\r\n")] {
        let value: serde_json::Value = serde_yaml::from_str(&input).unwrap();
        assert_eq!(value, expected);
        let keys: Vec<_> = value["parameters"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "enabled",
                "literal",
                "code",
                "absent",
                "names",
                "description"
            ]
        );
        let serialized = serde_yaml::to_string(&value).unwrap();
        let reparsed: serde_json::Value = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed, expected);
    }
}

#[test]
fn yaml_backend_reports_malformed_input_locations() {
    let error = serde_yaml::from_str::<serde_json::Value>("groups: [unterminated").unwrap_err();
    let location = error
        .location()
        .expect("YAML errors retain source locations");
    assert!(location.line() >= 1);
    assert!(location.column() >= 1);
}
