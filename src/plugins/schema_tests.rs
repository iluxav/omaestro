//! Tests of `plugins::schema`.
use super::*;
use serde_json::json;

fn opt(kind: Kind) -> Opt {
    Opt {
        key: "x".into(),
        kind,
        label: None,
        description: None,
        default: None,
        optional: false,
        options: vec!["left".into(), "right".into()],
        keys: vec!["Left".into(), "Right".into()],
    }
}

#[test]
fn answers_are_read_by_type() {
    assert_eq!(
        opt(Kind::Chord).parse("super+alt+j"),
        Ok(json!("SUPER + ALT + J"))
    );
    assert!(opt(Kind::Chord).parse("HYPER + J").is_err());
    assert_eq!(
        opt(Kind::Modifiers).parse("ctrl alt"),
        Ok(json!("CTRL + ALT + "))
    );
    assert_eq!(
        opt(Kind::Modifiers).parse("CTRL + ALT + "),
        Ok(json!("CTRL + ALT + "))
    );
    assert_eq!(opt(Kind::Bool).parse("Yes"), Ok(json!(true)));
    assert_eq!(opt(Kind::Number).parse("15"), Ok(json!(15)));
    assert_eq!(opt(Kind::Number).parse("0.5"), Ok(json!(0.5)));
    assert_eq!(opt(Kind::Interval).parse("1h30m"), Ok(json!("1h30m")));
    assert!(opt(Kind::Interval).parse("soon").is_err());
    assert_eq!(opt(Kind::Time).parse("09:00"), Ok(json!("09:00")));
    assert!(
        opt(Kind::Enum)
            .parse("up")
            .unwrap_err()
            .contains("left, right")
    );
    assert!(opt(Kind::String).parse("  ").is_err());
    // none means off where the option allows it
    let optional = |kind| Opt {
        optional: true,
        ..opt(kind)
    };
    assert_eq!(optional(Kind::Chord).parse("none"), Ok(json!(false)));
    assert_eq!(optional(Kind::String).parse("-"), Ok(Json::Null));
}

#[test]
fn values_become_lua() {
    assert_eq!(
        lua_literal(&json!("SUPER + ALT + J")),
        "\"SUPER + ALT + J\""
    );
    assert_eq!(lua_literal(&json!("a \"b\"\n")), "\"a \\\"b\\\"\\n\"");
    assert_eq!(lua_literal(&json!(false)), "false");
    assert_eq!(lua_literal(&json!(10)), "10");
    assert_eq!(lua_key("chord"), "chord");
    assert_eq!(lua_key("battery-low"), "[\"battery-low\"]");
}

#[test]
fn chords_of_a_value() {
    let names: Vec<String> = opt(Kind::Modifiers)
        .chords(&json!("CTRL + ALT + "))
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert_eq!(names, ["CTRL + ALT + Left", "CTRL + ALT + Right"]);
    assert!(opt(Kind::Chord).chords(&json!(false)).is_empty());
}

#[test]
fn a_schema_is_checked_when_read() {
    let dir = crate::testutil::TempDir::new("schema");
    let write = |text: &str| std::fs::write(dir.path().join(FILE), text).unwrap();
    assert_eq!(load(dir.path()).unwrap(), None);
    write(
        r#"{ "options": [ { "key": "chord", "type": "chord", "default": "SUPER + ALT + P" } ] }"#,
    );
    assert_eq!(load(dir.path()).unwrap().unwrap().options.len(), 1);
    write(r#"{ "options": [ { "key": "n", "type": "number", "default": "ten" } ] }"#);
    assert!(
        load(dir.path())
            .unwrap_err()
            .to_string()
            .contains("does not fit")
    );
    write(r#"{ "options": [ { "key": "a", "type": "bool" }, { "key": "a", "type": "bool" } ] }"#);
    assert!(load(dir.path()).unwrap_err().to_string().contains("twice"));
    write(r#"{ "options": [ { "key": "e", "type": "enum" } ] }"#);
    assert!(
        load(dir.path())
            .unwrap_err()
            .to_string()
            .contains("without options")
    );
    write("{ nope");
    assert!(load(dir.path()).is_err());
}
