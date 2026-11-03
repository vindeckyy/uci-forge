//! Build minimal valid examples from the schema.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use uci_schema::{MaxOccurs, Schema, TypeInfo};

/// Build a validated example message, or `None` when the shape cannot be
/// generated without inventing required data.
pub fn message(schema: &Schema, name: &str) -> Option<Value> {
    let value = message_unchecked(schema, name)?;
    if uci_validate::validate(schema, &value).is_ok() {
        Some(value)
    } else {
        None
    }
}

/// Build an example without validating it. Useful for diagnostics.
pub fn message_unchecked(schema: &Schema, name: &str) -> Option<Value> {
    let info = schema.messages.get(name)?;
    let mut stack = Vec::new();
    let payload = sample_type(schema, &info.root_type, name, 0, &mut stack)?;
    let mut root = Map::new();
    root.insert(name.to_owned(), payload);
    Some(Value::Object(root))
}

fn sample_type(
    schema: &Schema,
    type_name: &str,
    hint: &str,
    depth: usize,
    stack: &mut Vec<String>,
) -> Option<Value> {
    if depth > 16 || stack.iter().any(|n| n == type_name) {
        return None;
    }
    if let Some(TypeInfo::Complex(c)) = schema.types.get(type_name) {
        if c.abstract_ {
            let subtype = schema
                .subtypes(type_name)
                .iter()
                .find(|s| schema.complex(s).is_some_and(|c| !c.abstract_))?;
            let mut value = sample_type(schema, subtype, hint, depth + 1, stack)?;
            if let Value::Object(object) = &mut value {
                object.insert("$type".to_owned(), Value::String(subtype.clone()));
                return Some(value);
            }
            return None;
        }

        stack.push(type_name.to_owned());
        let choices = schema.choices(type_name);
        let mut chosen: BTreeSet<String> = BTreeSet::new();
        let mut skipped: BTreeSet<String> = BTreeSet::new();
        for choice in &choices {
            if let Some(first) = choice.members.first() {
                chosen.insert(first.clone());
            }
            for member in choice.members.iter().skip(1) {
                skipped.insert(member.clone());
            }
        }
        let mut object = Map::new();
        for field in schema.fields(type_name) {
            if skipped.contains(&field.name) {
                continue;
            }
            let required = field.min_occurs > 0
                && !choices.iter().any(|c| c.members.contains(&field.name))
                || chosen.contains(&field.name);
            if !required {
                continue;
            }
            let value = sample_type(schema, &field.type_name, &field.name, depth + 1, stack);
            let Some(value) = value else {
                stack.pop();
                return None;
            };
            let value = match field.max_occurs {
                MaxOccurs::One => value,
                MaxOccurs::Bounded { .. } | MaxOccurs::Unbounded => Value::Array(vec![value]),
            };
            object.insert(field.name.clone(), value);
        }
        stack.pop();
        return Some(Value::Object(object));
    }

    if let Some(TypeInfo::Simple(_)) = schema.types.get(type_name) {
        let values = schema.enum_values(type_name);
        if let Some(first) = values.first() {
            return Some(Value::String(first.clone()));
        }
        let builtin = schema.simple_builtin(type_name);
        return sample_builtin(schema, type_name, &builtin, hint);
    }

    sample_builtin(schema, type_name, type_name, hint)
}

fn sample_builtin(schema: &Schema, type_name: &str, builtin: &str, hint: &str) -> Option<Value> {
    match builtin {
        "boolean" => Some(Value::Bool(false)),
        "int" | "integer" | "long" | "short" | "byte" | "nonPositiveInteger"
        | "negativeInteger" | "unsignedInt" | "unsignedLong" | "unsignedShort" | "unsignedByte"
        | "positiveInteger" | "nonNegativeInteger" => Some(numeric(schema, type_name, 0)),
        "double" | "float" | "decimal" => Some(numeric_f64(schema, type_name, 0.0)),
        "dateTime" | "date" | "time" => Some(Value::String("2026-11-02T12:00:00Z".to_owned())),
        "hexBinary" => Some(Value::String("00".to_owned())),
        _ => {
            let facets = all_facets(schema, type_name);
            if facets.is_empty() {
                return Some(Value::String(string_hint(hint)));
            }
            let mut candidates = vec![
                string_hint(hint),
                "123e4567-e89b-12d3-a456-426614174000".to_owned(),
                "002.6.0".to_owned(),
                "2026-11-02T12:00:00Z".to_owned(),
                "EAGLE".to_owned(),
                "192.168.0.1".to_owned(),
                "AB-2026".to_owned(),
                "AB-2026T".to_owned(),
                "ABCAZ12345".to_owned(),
                "example".to_owned(),
                "ID-1".to_owned(),
                "A".to_owned(),
                "0".to_owned(),
                String::new(),
            ];
            candidates.dedup();
            for candidate in candidates {
                if string_ok(&facets, &candidate) {
                    return Some(Value::String(candidate));
                }
            }
            None
        }
    }
}

fn numeric(schema: &Schema, type_name: &str, fallback: i64) -> Value {
    let mut value = fallback;
    for facet in all_facets(schema, type_name) {
        match facet.kind.as_str() {
            "minInclusive" => {
                if let Ok(min) = facet.value.parse::<i64>() {
                    value = value.max(min);
                }
            }
            "maxInclusive" => {
                if let Ok(max) = facet.value.parse::<i64>() {
                    value = value.min(max);
                }
            }
            _ => {}
        }
    }
    Value::from(value)
}

fn numeric_f64(schema: &Schema, type_name: &str, fallback: f64) -> Value {
    let mut value = fallback;
    for facet in all_facets(schema, type_name) {
        match facet.kind.as_str() {
            "minInclusive" => {
                if let Ok(min) = facet.value.parse::<f64>() {
                    value = value.max(min);
                }
            }
            "maxInclusive" => {
                if let Ok(max) = facet.value.parse::<f64>() {
                    value = value.min(max);
                }
            }
            _ => {}
        }
    }
    Value::from(value)
}

fn all_facets(schema: &Schema, type_name: &str) -> Vec<uci_schema::Facet> {
    let mut out = Vec::new();
    let mut current = Some(type_name.to_owned());
    let mut guard = 0usize;
    while let Some(name) = current {
        guard += 1;
        if guard > 32 {
            break;
        }
        let Some(simple) = schema.simple(&name) else {
            break;
        };
        out.extend(simple.facets.clone());
        current = Some(simple.base.clone());
    }
    out
}

fn string_ok(facets: &[uci_schema::Facet], value: &str) -> bool {
    for facet in facets {
        match facet.kind.as_str() {
            "minLength" => {
                if let Ok(limit) = facet.value.parse::<usize>()
                    && value.chars().count() < limit
                {
                    return false;
                }
            }
            "maxLength" => {
                if let Ok(limit) = facet.value.parse::<usize>()
                    && value.chars().count() > limit
                {
                    return false;
                }
            }
            "length" => {
                if let Ok(limit) = facet.value.parse::<usize>()
                    && value.chars().count() != limit
                {
                    return false;
                }
            }
            "pattern" => {
                if let Ok(pattern) = regex::Regex::new(&facet.value)
                    && !pattern.is_match(value)
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn string_hint(hint: &str) -> String {
    let lower = hint.to_ascii_lowercase();
    if lower.contains("timestamp") || lower.ends_with("time") {
        "2026-11-02T12:00:00Z".to_owned()
    } else if lower.contains("version") {
        "002.6.0".to_owned()
    } else if lower.contains("callsign") {
        "EAGLE".to_owned()
    } else if lower.contains("uuid") || lower.ends_with("id") {
        "123e4567-e89b-12d3-a456-426614174000".to_owned()
    } else {
        "example".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mini() -> Schema {
        Schema::load_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
        )
        .expect("load mini schema")
    }

    #[test]
    fn builds_mini_example() {
        let value = message(&mini(), "MiniMessage").expect("example");
        assert!(value.get("MiniMessage").is_some());
        assert_eq!(
            value["MiniMessage"]["Payload"]["$type"],
            Value::String("TrackPayloadType".to_owned())
        );
    }

    #[test]
    fn unknown_message_returns_none() {
        assert!(message(&mini(), "Nope").is_none());
    }

    fn sample(type_name: &str, hint: &str) -> Option<Value> {
        sample_type(&Schema::default(), type_name, hint, 0, &mut Vec::new())
    }

    #[test]
    fn samples_every_builtin() {
        assert_eq!(sample("boolean", "x"), Some(Value::Bool(false)));
        assert_eq!(sample("int", "x"), Some(Value::from(0)));
        assert_eq!(sample("unsignedLong", "x"), Some(Value::from(0)));
        assert_eq!(sample("double", "x"), Some(Value::from(0.0)));
        assert_eq!(
            sample("hexBinary", "x"),
            Some(Value::String("00".to_owned()))
        );
        assert_eq!(
            sample("dateTime", "x"),
            Some(Value::String("2026-11-02T12:00:00Z".to_owned()))
        );
        assert_eq!(
            sample("string", "Timestamp"),
            Some(Value::String("2026-11-02T12:00:00Z".to_owned()))
        );
        assert_eq!(
            sample("Custom", "x"),
            Some(Value::String("example".to_owned()))
        );
    }

    #[test]
    fn samples_enum_first_value() {
        assert_eq!(
            sample_type(&mini(), "ModeEnum", "Mode", 0, &mut Vec::new()),
            Some(Value::String("AUTO".to_owned()))
        );
    }

    #[test]
    fn skips_optional_and_unchosen_choice_fields() {
        let value = message_unchecked(&mini(), "MiniMessage").expect("example");
        let object = value["MiniMessage"].as_object().unwrap();
        assert!(!object.contains_key("Note"));
        assert!(!object.contains_key("Callsign"));
        assert!(object["Choice"].get("Beta").is_none());
    }

    #[test]
    fn repeats_required_arrays() {
        let mut schema = mini();
        let Some(TypeInfo::Complex(complex)) = schema.types.get_mut("MiniMessageMT") else {
            panic!("MiniMessageMT");
        };
        if let uci_schema::Content::Sequence { elements } = &mut complex.content {
            let note = elements.iter_mut().find(|e| e.name == "Note").unwrap();
            note.min_occurs = 1;
        }
        let value = message_unchecked(&schema, "MiniMessage").expect("example");
        assert!(value["MiniMessage"]["Note"].is_array());
    }

    #[test]
    fn bounds_required_arrays() {
        let mut schema = mini();
        let Some(TypeInfo::Complex(complex)) = schema.types.get_mut("MiniMessageMT") else {
            panic!("MiniMessageMT");
        };
        if let uci_schema::Content::Sequence { elements } = &mut complex.content {
            let note = elements.iter_mut().find(|e| e.name == "Note").unwrap();
            note.min_occurs = 1;
            note.max_occurs = MaxOccurs::Bounded { n: 2 };
        }
        let value = message_unchecked(&schema, "MiniMessage").expect("example");
        assert!(value["MiniMessage"]["Note"].is_array());
    }

    #[test]
    fn returns_none_past_depth_limit() {
        assert!(sample_type(&Schema::default(), "string", "x", 17, &mut Vec::new()).is_none());
    }

    #[test]
    fn returns_none_on_cycle() {
        let mut stack = vec!["A".to_owned()];
        assert!(sample_type(&Schema::default(), "A", "x", 0, &mut stack).is_none());
    }

    #[test]
    fn returns_none_for_abstract_without_subtype() {
        let mut schema = Schema::default();
        schema.types.insert(
            "A".to_owned(),
            TypeInfo::Complex(uci_schema::ComplexType {
                name: "A".to_owned(),
                abstract_: true,
                docs: String::new(),
                base: None,
                content: uci_schema::Content::Empty,
                subtypes: Vec::new(),
            }),
        );
        assert!(sample_type(&schema, "A", "x", 0, &mut Vec::new()).is_none());
    }

    #[test]
    fn returns_none_when_subtype_is_not_object() {
        let mut schema = Schema::default();
        schema.types.insert(
            "A".to_owned(),
            TypeInfo::Complex(uci_schema::ComplexType {
                name: "A".to_owned(),
                abstract_: true,
                docs: String::new(),
                base: None,
                content: uci_schema::Content::Empty,
                subtypes: vec!["S".to_owned()],
            }),
        );
        schema.types.insert(
            "S".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "S".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: Vec::new(),
            }),
        );
        assert!(sample_type(&schema, "A", "x", 0, &mut Vec::new()).is_none());
    }

    #[test]
    fn returns_none_when_validation_fails() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Score".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Score".to_owned(),
                base: "dateTime".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![uci_schema::Facet {
                    kind: "pattern".to_owned(),
                    value: "^never$".to_owned(),
                }],
            }),
        );
        schema.types.insert(
            "MsgMT".to_owned(),
            TypeInfo::Complex(uci_schema::ComplexType {
                name: "MsgMT".to_owned(),
                abstract_: false,
                docs: String::new(),
                base: None,
                content: uci_schema::Content::Sequence {
                    elements: vec![uci_schema::Element {
                        name: "Score".to_owned(),
                        type_name: "Score".to_owned(),
                        min_occurs: 1,
                        max_occurs: MaxOccurs::One,
                        docs: String::new(),
                    }],
                },
                subtypes: Vec::new(),
            }),
        );
        schema.messages.insert(
            "Msg".to_owned(),
            uci_schema::MessageInfo {
                name: "Msg".to_owned(),
                primitive: None,
                summary: String::new(),
                docs: String::new(),
                root_type: "MsgMT".to_owned(),
            },
        );
        schema.roots.insert("Msg".to_owned(), "MsgMT".to_owned());
        assert!(message_unchecked(&schema, "Msg").is_some());
        assert!(message(&schema, "Msg").is_none());
    }

    fn facet(kind: &str, value: &str) -> uci_schema::Facet {
        uci_schema::Facet {
            kind: kind.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn checks_string_facets() {
        assert!(!string_ok(&[facet("minLength", "3")], "ab"));
        assert!(!string_ok(&[facet("maxLength", "2")], "abc"));
        assert!(!string_ok(&[facet("length", "2")], "abc"));
        assert!(!string_ok(&[facet("pattern", "^[A-Z]+$")], "abc"));
        assert!(string_ok(&[facet("pattern", "^[a-z]+$")], "abc"));
        assert!(string_ok(&[facet("pattern", "(")], "abc"));
        assert!(string_ok(&[facet("minLength", "x")], "abc"));
        assert!(string_ok(&[facet("unknown", "x")], "abc"));
    }

    #[test]
    fn picks_a_candidate_that_fits() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Short".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Short".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![facet("maxLength", "1")],
            }),
        );
        assert_eq!(
            sample_type(&schema, "Short", "x", 0, &mut Vec::new()),
            Some(Value::String("A".to_owned()))
        );
    }

    #[test]
    fn returns_none_when_no_candidate_fits() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Tiny".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Tiny".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![facet("length", "3"), facet("pattern", "^[A-Z]{3}$")],
            }),
        );
        assert!(sample_type(&schema, "Tiny", "x", 0, &mut Vec::new()).is_none());
    }

    #[test]
    fn collects_facets_through_base_chain() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Base".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Base".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![facet("minLength", "2")],
            }),
        );
        schema.types.insert(
            "Child".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Child".to_owned(),
                base: "Base".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![facet("maxLength", "4")],
            }),
        );
        assert_eq!(all_facets(&schema, "Child").len(), 2);
    }

    #[test]
    fn stops_facet_walk_at_guard() {
        let mut schema = Schema::default();
        for i in 0..40 {
            let base = if i == 0 {
                "string".to_owned()
            } else {
                format!("T{}", i - 1)
            };
            schema.types.insert(
                format!("T{i}"),
                TypeInfo::Simple(uci_schema::SimpleType {
                    name: format!("T{i}"),
                    base,
                    docs: String::new(),
                    enum_values: Vec::new(),
                    facets: Vec::new(),
                }),
            );
        }
        assert!(all_facets(&schema, "T39").is_empty());
    }

    fn simple_type(name: &str, base: &str, facets: Vec<uci_schema::Facet>) -> TypeInfo {
        TypeInfo::Simple(uci_schema::SimpleType {
            name: name.to_owned(),
            base: base.to_owned(),
            docs: String::new(),
            enum_values: Vec::new(),
            facets,
        })
    }

    #[test]
    fn numeric_helpers_respect_bounds_and_ignore_other_facets() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Pos".to_owned(),
            simple_type("Pos", "unsignedInt", vec![facet("minInclusive", "1")]),
        );
        schema.types.insert(
            "Neg".to_owned(),
            simple_type("Neg", "int", vec![facet("maxInclusive", "-1")]),
        );
        schema.types.insert(
            "Odd".to_owned(),
            simple_type("Odd", "int", vec![facet("pattern", "x")]),
        );
        schema.types.insert(
            "Half".to_owned(),
            simple_type("Half", "double", vec![facet("minInclusive", "0.5")]),
        );
        schema.types.insert(
            "Ceiling".to_owned(),
            simple_type("Ceiling", "double", vec![facet("maxInclusive", "0.25")]),
        );
        schema.types.insert(
            "OddDouble".to_owned(),
            simple_type("OddDouble", "double", vec![facet("length", "1")]),
        );
        assert_eq!(numeric(&schema, "Pos", 0), Value::from(1));
        assert_eq!(numeric(&schema, "Neg", 0), Value::from(-1));
        assert_eq!(numeric(&schema, "Odd", 0), Value::from(0));
        assert_eq!(numeric_f64(&schema, "Half", 0.0), Value::from(0.5));
        assert_eq!(numeric_f64(&schema, "Ceiling", 1.0), Value::from(0.25));
        assert_eq!(numeric_f64(&schema, "OddDouble", 0.0), Value::from(0.0));
    }

    #[test]
    fn string_hints_are_specific() {
        assert_eq!(string_hint("Timestamp"), "2026-11-02T12:00:00Z");
        assert_eq!(string_hint("SchemaVersion"), "002.6.0");
        assert_eq!(string_hint("Callsign"), "EAGLE");
        assert_eq!(
            string_hint("SystemID"),
            "123e4567-e89b-12d3-a456-426614174000"
        );
        assert_eq!(string_hint("Name"), "example");
    }
}
