//! Validate OMS JSON messages against a parsed UCI schema.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uci_schema::{Element, MaxOccurs, Schema};

/// One structural validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    /// JSON path to the failure.
    pub path: String,
    /// Stable error code.
    pub code: String,
    /// Expected shape, when useful.
    pub expected: Option<String>,
    /// Found shape, when useful.
    pub found: Option<String>,
}

impl ValidationError {
    fn new(path: &str, code: &str, expected: Option<String>, found: Option<String>) -> Self {
        Self {
            path: path.to_owned(),
            code: code.to_owned(),
            expected,
            found,
        }
    }
}

/// Validate one OMS JSON message. Returns the message name on success.
pub fn validate(schema: &Schema, value: &Value) -> Result<String, Vec<ValidationError>> {
    let Some(root) = value.as_object() else {
        return Err(vec![ValidationError::new(
            "$",
            "ROOT_SHAPE",
            Some("object with one message key".to_owned()),
            Some(type_of(value).to_owned()),
        )]);
    };
    if root.len() != 1 {
        return Err(vec![ValidationError::new(
            "$",
            "ROOT_SHAPE",
            Some("object with exactly one key".to_owned()),
            Some(format!("{} keys", root.len())),
        )]);
    }
    let (message_name, payload) = root.iter().next().expect("one key");
    let Some(root_type) = schema.roots.get(message_name) else {
        return Err(vec![ValidationError::new(
            "$",
            "UNKNOWN_MESSAGE",
            Some("known UCI message root".to_owned()),
            Some(message_name.clone()),
        )]);
    };

    let mut errors = Vec::new();
    validate_type(
        schema,
        root_type,
        payload,
        &format!("$.{message_name}"),
        &mut errors,
    );
    errors.truncate(50);
    if errors.is_empty() {
        Ok(message_name.clone())
    } else {
        Err(errors)
    }
}

/// Parse and validate a JSON string.
pub fn validate_str(schema: &Schema, text: &str) -> Result<String, Vec<ValidationError>> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        vec![ValidationError::new(
            "$",
            "ROOT_SHAPE",
            Some("valid JSON".to_owned()),
            Some(e.to_string()),
        )]
    })?;
    validate(schema, &value)
}

fn validate_type(
    schema: &Schema,
    type_name: &str,
    value: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(complex) = schema.complex(type_name) {
        let mut actual = type_name;
        if let Some(declared) = value.get("$type").and_then(Value::as_str) {
            let Some(sub) = schema.complex(declared) else {
                errors.push(ValidationError::new(
                    path,
                    "BAD_POLYMORPHIC_TYPE",
                    Some("known subtype in $type".to_owned()),
                    Some(declared.to_owned()),
                ));
                return;
            };
            if !derives_from(schema, declared, type_name) {
                errors.push(ValidationError::new(
                    path,
                    "BAD_POLYMORPHIC_TYPE",
                    Some(format!("subtype of {type_name}")),
                    Some(declared.to_owned()),
                ));
                return;
            }
            actual = sub.name.as_str();
        } else if complex.abstract_ {
            errors.push(ValidationError::new(
                path,
                "BAD_POLYMORPHIC_TYPE",
                Some("$type for abstract type".to_owned()),
                Some("absent".to_owned()),
            ));
            return;
        }
        validate_complex(schema, actual, value, path, errors);
        return;
    }

    if schema.simple(type_name).is_some() {
        validate_simple(schema, type_name, value, path, errors);
        return;
    }

    validate_builtin(type_name, value, path, errors);
}

fn validate_complex(
    schema: &Schema,
    type_name: &str,
    value: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(object) = value.as_object() else {
        errors.push(ValidationError::new(
            path,
            "TYPE_MISMATCH",
            Some("object".to_owned()),
            Some(type_of(value).to_owned()),
        ));
        return;
    };

    let fields: BTreeMap<String, Element> = schema
        .fields(type_name)
        .into_iter()
        .map(|f| (f.name.clone(), f))
        .collect();
    let choices = schema.choices(type_name);
    let mut choice_members: BTreeSet<String> = BTreeSet::new();
    for choice in &choices {
        choice_members.extend(choice.members.iter().cloned());
    }

    for key in object.keys() {
        if key == "$type" {
            continue;
        }
        if !fields.contains_key(key) {
            errors.push(ValidationError::new(
                &format!("{path}.{key}"),
                "UNKNOWN_MEMBER",
                Some("declared field".to_owned()),
                Some(key.clone()),
            ));
        }
    }

    for field in fields.values() {
        let present = object.contains_key(&field.name);
        if !present {
            if field.min_occurs > 0 && !choice_members.contains(&field.name) {
                errors.push(ValidationError::new(
                    &format!("{path}.{}", field.name),
                    "MISSING_REQUIRED",
                    Some(field.type_name.clone()),
                    Some("absent".to_owned()),
                ));
            }
            continue;
        }
        let child = &object[&field.name];
        validate_field(schema, field, child, path, errors);
    }

    for choice in &choices {
        let present = choice
            .members
            .iter()
            .filter(|name| object.contains_key(name.as_str()))
            .count();
        if (present as u32) < choice.min_occurs {
            errors.push(ValidationError::new(
                path,
                "CHOICE",
                Some(format!(
                    "at least {} of {:?}",
                    choice.min_occurs, choice.members
                )),
                Some(format!("{present} present")),
            ));
        }
        if choice.max_occurs == MaxOccurs::One && present > 1 {
            errors.push(ValidationError::new(
                path,
                "CHOICE",
                Some("at most one choice member".to_owned()),
                Some(format!("{present} present")),
            ));
        }
    }
}

fn validate_field(
    schema: &Schema,
    field: &Element,
    value: &Value,
    parent_path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let path = format!("{parent_path}.{}", field.name);
    match field.max_occurs {
        MaxOccurs::One => {
            if value.is_array() {
                errors.push(ValidationError::new(
                    &path,
                    "OCCURRENCE",
                    Some("single value".to_owned()),
                    Some("array".to_owned()),
                ));
                return;
            }
            validate_type(schema, &field.type_name, value, &path, errors);
        }
        MaxOccurs::Bounded { n } => {
            let Some(items) = value.as_array() else {
                errors.push(ValidationError::new(
                    &path,
                    "OCCURRENCE",
                    Some("array".to_owned()),
                    Some(type_of(value).to_owned()),
                ));
                return;
            };
            if items.len() > n as usize {
                errors.push(ValidationError::new(
                    &path,
                    "OCCURRENCE",
                    Some(format!("at most {n} items")),
                    Some(format!("{} items", items.len())),
                ));
            }
            validate_items(schema, field, items, &path, errors);
        }
        MaxOccurs::Unbounded => {
            let Some(items) = value.as_array() else {
                errors.push(ValidationError::new(
                    &path,
                    "OCCURRENCE",
                    Some("array".to_owned()),
                    Some(type_of(value).to_owned()),
                ));
                return;
            };
            validate_items(schema, field, items, &path, errors);
        }
    }
}

fn validate_items(
    schema: &Schema,
    field: &Element,
    items: &[Value],
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    if field.min_occurs > 0 && items.is_empty() {
        errors.push(ValidationError::new(
            path,
            "OCCURRENCE",
            Some(format!("at least {} items", field.min_occurs)),
            Some("0 items".to_owned()),
        ));
    }
    for (index, item) in items.iter().enumerate() {
        validate_type(
            schema,
            &field.type_name,
            item,
            &format!("{path}[{index}]"),
            errors,
        );
    }
}

fn validate_simple(
    schema: &Schema,
    type_name: &str,
    value: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let builtin = schema.simple_builtin(type_name);
    validate_builtin(&builtin, value, path, errors);

    let enum_values = schema.enum_values(type_name);
    if !enum_values.is_empty() {
        let Some(text) = value.as_str() else {
            errors.push(ValidationError::new(
                path,
                "TYPE_MISMATCH",
                Some("string enumeration".to_owned()),
                Some(type_of(value).to_owned()),
            ));
            return;
        };
        if !enum_values.iter().any(|v| v == text) {
            errors.push(ValidationError::new(
                path,
                "FACET",
                Some(format!("one of {enum_values:?}")),
                Some(text.to_owned()),
            ));
        }
    }

    if let Some(simple) = schema.simple(type_name) {
        for facet in &simple.facets {
            apply_facet(facet, value, path, errors);
        }
    }
}

fn apply_facet(
    facet: &uci_schema::Facet,
    value: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    match facet.kind.as_str() {
        "minLength" | "maxLength" | "length" => {
            let Some(text) = value.as_str() else {
                return;
            };
            let Ok(limit) = facet.value.parse::<usize>() else {
                return;
            };
            let len = text.chars().count();
            let bad = match facet.kind.as_str() {
                "minLength" => len < limit,
                "maxLength" => len > limit,
                _ => len != limit,
            };
            if bad {
                errors.push(ValidationError::new(
                    path,
                    "FACET",
                    Some(format!("{} {}", facet.kind, facet.value)),
                    Some(format!("length {len}")),
                ));
            }
        }
        "minInclusive" | "maxInclusive" => {
            let Some(number) = value.as_f64() else {
                return;
            };
            let Ok(limit) = facet.value.parse::<f64>() else {
                return;
            };
            let bad = match facet.kind.as_str() {
                "minInclusive" => number < limit,
                _ => number > limit,
            };
            if bad {
                errors.push(ValidationError::new(
                    path,
                    "FACET",
                    Some(format!("{} {}", facet.kind, facet.value)),
                    Some(number.to_string()),
                ));
            }
        }
        "pattern" => {
            let Some(text) = value.as_str() else {
                return;
            };
            let Ok(pattern) = regex::Regex::new(&facet.value) else {
                return;
            };
            if !pattern.is_match(text) {
                errors.push(ValidationError::new(
                    path,
                    "FACET",
                    Some(format!("pattern {}", facet.value)),
                    Some(text.to_owned()),
                ));
            }
        }
        _ => {}
    }
}

fn validate_builtin(name: &str, value: &Value, path: &str, errors: &mut Vec<ValidationError>) {
    let ok = match name {
        "boolean" => value.is_boolean(),
        "int" | "integer" | "long" | "short" | "byte" | "unsignedInt" | "unsignedLong"
        | "unsignedShort" | "unsignedByte" | "positiveInteger" | "nonNegativeInteger"
        | "negativeInteger" | "nonPositiveInteger" => {
            value.as_i64().is_some() || value.as_u64().is_some()
        }
        "double" | "float" | "decimal" => value.is_number(),
        "dateTime" | "date" | "time" => value.as_str().is_some_and(is_datetime_like),
        "hexBinary" => value
            .as_str()
            .is_some_and(|s| s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        _ => value.is_string(),
    };
    if !ok {
        errors.push(ValidationError::new(
            path,
            "TYPE_MISMATCH",
            Some(name.to_owned()),
            Some(type_of(value).to_owned()),
        ));
    }
}

fn is_datetime_like(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn derives_from(schema: &Schema, child: &str, base: &str) -> bool {
    if child == base {
        return true;
    }
    let mut current = schema.complex(child).and_then(|c| c.base.clone());
    let mut guard = 0usize;
    while let Some(name) = current {
        guard += 1;
        if guard > 64 {
            return false;
        }
        if name == base {
            return true;
        }
        current = schema.complex(&name).and_then(|c| c.base.clone());
    }
    false
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Schema {
        Schema::load_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
        )
        .expect("load mini schema")
    }

    fn valid() -> Value {
        json!({
            "MiniMessage": {
                "MessageHeader": {
                    "MessageID": "m-1",
                    "Timestamp": "2026-11-02T12:00:00Z"
                },
                "Payload": {
                    "$type": "TrackPayloadType",
                    "Kind": "track",
                    "TrackID": "t-1",
                    "Mode": "AUTO"
                },
                "Note": ["a", "b"],
                "Choice": { "Alpha": "x" },
                "Callsign": "EAGLE"
            }
        })
    }

    #[test]
    fn accepts_valid_message() {
        assert_eq!(validate(&schema(), &valid()).unwrap(), "MiniMessage");
    }

    #[test]
    fn rejects_non_object_root() {
        let errors = validate(&schema(), &json!([])).unwrap_err();
        assert_eq!(errors[0].code, "ROOT_SHAPE");
    }

    #[test]
    fn rejects_multiple_keys() {
        let errors = validate(&schema(), &json!({"A": {}, "B": {}})).unwrap_err();
        assert_eq!(errors[0].code, "ROOT_SHAPE");
    }

    #[test]
    fn rejects_unknown_message() {
        let errors = validate(&schema(), &json!({"Nope": {}})).unwrap_err();
        assert_eq!(errors[0].code, "UNKNOWN_MESSAGE");
    }

    #[test]
    fn rejects_non_object_payload() {
        let mut value = valid();
        value["MiniMessage"]["MessageHeader"] = json!("no");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }

    #[test]
    fn rejects_missing_required() {
        let mut value = valid();
        value["MiniMessage"]
            .as_object_mut()
            .unwrap()
            .remove("MessageHeader");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "MISSING_REQUIRED"));
    }

    #[test]
    fn rejects_unknown_member() {
        let mut value = valid();
        value["MiniMessage"]["Bogus"] = json!(1);
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "UNKNOWN_MEMBER"));
    }

    #[test]
    fn rejects_wrong_scalar_type() {
        let mut value = valid();
        value["MiniMessage"]["MessageHeader"]["MessageID"] = json!(7);
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }

    #[test]
    fn rejects_scalar_where_array_expected() {
        let mut value = valid();
        value["MiniMessage"]["Note"] = json!("a");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "OCCURRENCE"));
    }

    #[test]
    fn rejects_bad_array_item() {
        let mut value = valid();
        value["MiniMessage"]["Note"] = json!(["a", 2]);
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }

    #[test]
    fn rejects_both_choice_members() {
        let mut value = valid();
        value["MiniMessage"]["Choice"] = json!({"Alpha": "x", "Beta": 2});
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "CHOICE"));
    }

    #[test]
    fn rejects_empty_choice() {
        let mut value = valid();
        value["MiniMessage"]["Choice"] = json!({});
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "CHOICE"));
    }

    #[test]
    fn rejects_missing_polymorphic_type() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]
            .as_object_mut()
            .unwrap()
            .remove("$type");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "BAD_POLYMORPHIC_TYPE"));
    }

    #[test]
    fn rejects_unknown_polymorphic_type() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]["$type"] = json!("Nope");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "BAD_POLYMORPHIC_TYPE"));
    }

    #[test]
    fn rejects_non_derived_polymorphic_type() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]["$type"] = json!("ChoiceType");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "BAD_POLYMORPHIC_TYPE"));
    }

    #[test]
    fn rejects_unknown_enum_value() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]["Mode"] = json!("TURBO");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "FACET"));
    }

    #[test]
    fn rejects_bad_datetime() {
        let mut value = valid();
        value["MiniMessage"]["MessageHeader"]["Timestamp"] = json!("not-a-date");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }

    #[test]
    fn rejects_short_callsign() {
        let mut value = valid();
        value["MiniMessage"]["Callsign"] = json!("");
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "FACET"));
    }

    #[test]
    fn rejects_array_where_scalar_expected() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]["TrackID"] = json!(["t"]);
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "OCCURRENCE"));
    }

    #[test]
    fn validate_str_reports_invalid_json() {
        let errors = validate_str(&schema(), "{").unwrap_err();
        assert_eq!(errors[0].code, "ROOT_SHAPE");
    }

    fn with_note(max: MaxOccurs, min: u32) -> Schema {
        let mut schema = schema();
        let Some(uci_schema::TypeInfo::Complex(complex)) = schema.types.get_mut("MiniMessageMT")
        else {
            panic!("MiniMessageMT");
        };
        if let uci_schema::Content::Sequence { elements } = &mut complex.content {
            let note = elements.iter_mut().find(|e| e.name == "Note").unwrap();
            note.max_occurs = max;
            note.min_occurs = min;
        }
        schema
    }

    #[test]
    fn rejects_too_many_bounded_items() {
        let schema = with_note(MaxOccurs::Bounded { n: 2 }, 0);
        let mut value = valid();
        value["MiniMessage"]["Note"] = json!(["a", "b", "c"]);
        let errors = validate(&schema, &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "OCCURRENCE"));
    }

    #[test]
    fn rejects_scalar_for_bounded_field() {
        let schema = with_note(MaxOccurs::Bounded { n: 2 }, 0);
        let mut value = valid();
        value["MiniMessage"]["Note"] = json!("a");
        let errors = validate(&schema, &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "OCCURRENCE"));
    }

    #[test]
    fn rejects_empty_required_array() {
        let schema = with_note(MaxOccurs::Unbounded, 1);
        let mut value = valid();
        value["MiniMessage"]["Note"] = json!([]);
        let errors = validate(&schema, &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "OCCURRENCE"));
    }

    #[test]
    fn rejects_non_string_enum_value() {
        let mut value = valid();
        value["MiniMessage"]["Payload"]["Mode"] = json!(7);
        let errors = validate(&schema(), &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }

    fn facet(kind: &str, value: &str) -> uci_schema::Facet {
        uci_schema::Facet {
            kind: kind.to_owned(),
            value: value.to_owned(),
        }
    }

    fn facet_errors(facet: uci_schema::Facet, value: Value) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        apply_facet(&facet, &value, "$.x", &mut errors);
        errors
    }

    #[test]
    fn facet_length_branches() {
        assert!(!facet_errors(facet("minLength", "3"), json!("ab")).is_empty());
        assert!(!facet_errors(facet("maxLength", "2"), json!("abc")).is_empty());
        assert!(!facet_errors(facet("length", "2"), json!("abc")).is_empty());
        assert!(facet_errors(facet("length", "3"), json!("abc")).is_empty());
        assert!(facet_errors(facet("minLength", "3"), json!(7)).is_empty());
        assert!(facet_errors(facet("minLength", "x"), json!("abc")).is_empty());
    }

    #[test]
    fn facet_numeric_branches() {
        assert!(!facet_errors(facet("minInclusive", "10"), json!(9)).is_empty());
        assert!(!facet_errors(facet("maxInclusive", "10"), json!(11)).is_empty());
        assert!(facet_errors(facet("minInclusive", "10"), json!(10)).is_empty());
        assert!(facet_errors(facet("minInclusive", "10"), json!("x")).is_empty());
        assert!(facet_errors(facet("minInclusive", "x"), json!(1)).is_empty());
    }

    #[test]
    fn facet_pattern_branches() {
        let errors = facet_errors(facet("pattern", "^[A-Z]+$"), json!("abc"));
        assert_eq!(errors.len(), 1);
        assert!(facet_errors(facet("pattern", "^[a-z]+$"), json!("abc")).is_empty());
        assert!(facet_errors(facet("pattern", "^[a-z]+$"), json!(7)).is_empty());
        assert!(facet_errors(facet("pattern", "("), json!("abc")).is_empty());
        assert!(facet_errors(facet("unknown", "x"), json!("abc")).is_empty());
    }

    #[test]
    fn builtin_branches() {
        let mut errors = Vec::new();
        validate_builtin("boolean", &json!(true), "$", &mut errors);
        validate_builtin("boolean", &json!("true"), "$", &mut errors);
        validate_builtin("int", &json!(1), "$", &mut errors);
        validate_builtin("int", &json!("1"), "$", &mut errors);
        validate_builtin("double", &json!(1.5), "$", &mut errors);
        validate_builtin("double", &json!("1.5"), "$", &mut errors);
        validate_builtin("dateTime", &json!("2026-11-02T12:00:00Z"), "$", &mut errors);
        validate_builtin("dateTime", &json!("bad"), "$", &mut errors);
        validate_builtin("hexBinary", &json!("00ff"), "$", &mut errors);
        validate_builtin("hexBinary", &json!("0"), "$", &mut errors);
        validate_builtin("hexBinary", &json!("zz"), "$", &mut errors);
        validate_builtin("hexBinary", &json!(7), "$", &mut errors);
        validate_builtin("Custom", &json!("x"), "$", &mut errors);
        validate_builtin("Custom", &json!(7), "$", &mut errors);
        assert_eq!(errors.len(), 8, "{errors:?}");
    }

    #[test]
    fn derives_from_handles_chains_and_misses() {
        let schema = schema();
        assert!(derives_from(&schema, "TrackPayloadType", "PayloadType"));
        assert!(derives_from(
            &schema,
            "TrackPayloadType",
            "TrackPayloadType"
        ));
        assert!(!derives_from(&schema, "TrackPayloadType", "MiniMessageMT"));
        assert!(!derives_from(&schema, "Missing", "PayloadType"));
    }

    #[test]
    fn derives_from_stops_long_chains() {
        let mut schema = Schema::default();
        for i in 0..70 {
            let base = if i == 0 {
                None
            } else {
                Some(format!("T{}", i - 1))
            };
            schema.types.insert(
                format!("T{i}"),
                uci_schema::TypeInfo::Complex(uci_schema::ComplexType {
                    name: format!("T{i}"),
                    abstract_: false,
                    docs: String::new(),
                    base,
                    content: uci_schema::Content::Empty,
                    subtypes: Vec::new(),
                }),
            );
        }
        assert!(!derives_from(&schema, "T69", "Nope"));
        assert!(derives_from(&schema, "T69", "T10"));
        assert!(!derives_from(&schema, "T69", "T0"));
    }

    #[test]
    fn type_names_cover_all_json_kinds() {
        assert_eq!(type_of(&Value::Null), "null");
        assert_eq!(type_of(&json!(true)), "boolean");
        assert_eq!(type_of(&json!(1)), "number");
        assert_eq!(type_of(&json!("x")), "string");
        assert_eq!(type_of(&json!([])), "array");
        assert_eq!(type_of(&json!({})), "object");
    }

    #[test]
    fn unknown_complex_type_falls_back_to_string() {
        let mut schema = schema();
        let Some(uci_schema::TypeInfo::Complex(complex)) = schema.types.get_mut("MiniMessageMT")
        else {
            panic!("MiniMessageMT");
        };
        if let uci_schema::Content::Sequence { elements } = &mut complex.content {
            let callsign = elements.iter_mut().find(|e| e.name == "Callsign").unwrap();
            callsign.type_name = "Mystery".to_owned();
        }
        let mut value = valid();
        value["MiniMessage"]["Callsign"] = json!(7);
        let errors = validate(&schema, &value).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "TYPE_MISMATCH"));
    }
}
