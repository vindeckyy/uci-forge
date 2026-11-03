//! Generate typed Rust bindings from a parsed UCI schema.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use heck::{ToPascalCase, ToSnakeCase};
use uci_schema::{Schema, TypeInfo};

/// Messages used by the first generated crate.
pub const DEFAULT_MESSAGES: &[&str] = &[
    "AirReport",
    "AirfieldReport",
    "Entity",
    "EntityLost",
    "ESM_Activity",
    "MissionActivationRequest",
    "MissionActivationRequestStatus",
    "NavigationReport",
    "ObservationMeasurementReport",
    "OperatorNotification",
    "PO_Activity",
    "PositionReport",
    "PositionReportDetailed",
    "ServiceStatus",
    "SignalReport",
    "SubsystemStatus",
    "SystemStatus",
    "Task",
    "TaskCommand",
    "TaskCommandStatus",
];

/// A generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Relative path.
    pub path: String,
    /// File contents.
    pub contents: String,
}

/// A code generation failure with a stable reason.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodegenError {
    /// No message names were supplied.
    #[error("no messages selected")]
    NoMessages,
    /// A requested message does not exist in the schema.
    #[error("unknown message: {0}")]
    UnknownMessage(String),
    /// A field references a type that is neither declared nor a known builtin.
    #[error("unknown type {type_name} in {owner}.{field}")]
    UnknownType {
        /// Owning complex type.
        owner: String,
        /// Field name.
        field: String,
        /// Missing type name.
        type_name: String,
    },
}

/// Mapping from UCI names to Rust names.
#[derive(Debug, Default)]
struct Namer {
    types: BTreeMap<String, String>,
    used: BTreeSet<String>,
}

impl Namer {
    fn build(complex: &BTreeSet<String>, enums: &BTreeSet<String>) -> Self {
        let mut namer = Self::default();
        let mut names: Vec<&String> = complex.iter().chain(enums.iter()).collect();
        names.sort();
        for name in names {
            let base = sanitize_type(name);
            let mut candidate = base.clone();
            let mut suffix = 2u32;
            while !namer.used.insert(candidate.clone()) {
                candidate = format!("{base}{suffix}");
                suffix += 1;
            }
            namer.types.insert(name.clone(), candidate);
        }
        namer
    }

    fn type_name(&self, uci: &str) -> String {
        self.types
            .get(uci)
            .cloned()
            .unwrap_or_else(|| sanitize_type(uci))
    }
}

/// Generate the crate files for the selected messages.
pub fn generate(
    schema: &Schema,
    message_names: &[String],
) -> Result<Vec<GeneratedFile>, CodegenError> {
    if message_names.is_empty() {
        return Err(CodegenError::NoMessages);
    }
    for name in message_names {
        if !schema.messages.contains_key(name) {
            return Err(CodegenError::UnknownMessage(name.clone()));
        }
    }
    let roots: Vec<String> = message_names
        .iter()
        .filter_map(|name| schema.messages.get(name).map(|m| m.root_type.clone()))
        .collect();
    let mut all_types = schema.reachable_types(&roots);
    loop {
        let mut added = false;
        let snapshot: Vec<String> = all_types.iter().cloned().collect();
        for name in snapshot {
            for field in schema.fields(&name) {
                if schema.complex(&field.type_name).is_some()
                    && all_types.insert(field.type_name.clone())
                {
                    added = true;
                }
            }
            for subtype in schema.subtypes(&name) {
                if all_types.insert(subtype.clone()) {
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    validate_types(schema, &all_types)?;

    let mut complex = BTreeSet::new();
    let mut enums = BTreeSet::new();

    for name in &all_types {
        if let Some(info) = schema.types.get(name)
            && let TypeInfo::Complex(c) = info
        {
            if !c.abstract_ {
                complex.insert(name.clone());
            }
            for field in schema.fields(name) {
                collect_simple(schema, &field.type_name, &mut enums);
            }
        }
    }
    // Message roots must always be generated, even if a root is abstract.
    for root in &roots {
        if schema.complex(root).is_some() {
            complex.insert(root.clone());
        }
    }
    for name in &complex {
        for field in schema.fields(name) {
            collect_simple(schema, &field.type_name, &mut enums);
        }
    }

    let namer = Namer::build(&complex, &enums);
    let boxed = boxed_edges(schema, &complex);

    Ok(vec![
        GeneratedFile {
            path: "Cargo.toml".to_owned(),
            contents: generated_manifest(),
        },
        GeneratedFile {
            path: "src/lib.rs".to_owned(),
            contents: generated_lib(),
        },
        GeneratedFile {
            path: "src/types.rs".to_owned(),
            contents: render_types(schema, &complex, &enums, &namer, &boxed),
        },
        GeneratedFile {
            path: "tests/roundtrip.rs".to_owned(),
            contents: generated_tests(message_names.iter().any(|m| m == "TaskCommand")),
        },
    ])
}

/// Write generated files under a crate root.
pub fn write_files(root: &Path, files: &[GeneratedFile]) -> anyhow::Result<()> {
    for file in files {
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &file.contents)?;
    }
    Ok(())
}

fn validate_types(schema: &Schema, all: &BTreeSet<String>) -> Result<(), CodegenError> {
    for owner in all {
        for field in schema.fields(owner) {
            let known = schema.complex(&field.type_name).is_some()
                || schema.simple(&field.type_name).is_some()
                || is_builtin(&field.type_name);
            if !known {
                return Err(CodegenError::UnknownType {
                    owner: owner.clone(),
                    field: field.name.clone(),
                    type_name: field.type_name.clone(),
                });
            }
        }
    }
    Ok(())
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "boolean"
            | "int"
            | "integer"
            | "long"
            | "short"
            | "byte"
            | "unsignedInt"
            | "unsignedLong"
            | "unsignedShort"
            | "unsignedByte"
            | "positiveInteger"
            | "nonNegativeInteger"
            | "negativeInteger"
            | "nonPositiveInteger"
            | "double"
            | "float"
            | "decimal"
            | "dateTime"
            | "date"
            | "time"
            | "hexBinary"
            | "base64Binary"
            | "anyURI"
            | "normalizedString"
            | "token"
            | "Name"
            | "NCName"
            | "ID"
            | "IDREF"
            | "language"
            | "QName"
            | "duration"
            | "gYear"
            | "gMonth"
            | "gDay"
            | "gYearMonth"
            | "gMonthDay"
    )
}

fn collect_simple(schema: &Schema, name: &str, enums: &mut BTreeSet<String>) {
    if let Some(TypeInfo::Simple(s)) = schema.types.get(name) {
        if !schema.enum_values(name).is_empty() {
            enums.insert(name.to_owned());
        } else if schema.complex(&s.base).is_none() {
            collect_simple(schema, &s.base, enums);
        }
    }
}

fn boxed_edges(schema: &Schema, complex: &BTreeSet<String>) -> BTreeSet<(String, String)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    fn dfs(
        schema: &Schema,
        node: &str,
        complex: &BTreeSet<String>,
        colors: &mut BTreeMap<String, Color>,
        boxed: &mut BTreeSet<(String, String)>,
    ) {
        colors.insert(node.to_owned(), Color::Gray);
        for field in schema.fields(node) {
            if !complex.contains(&field.type_name) {
                continue;
            }
            match colors
                .get(&field.type_name)
                .copied()
                .unwrap_or(Color::White)
            {
                Color::White => {
                    dfs(schema, &field.type_name, complex, colors, boxed);
                }
                Color::Gray => {
                    boxed.insert((node.to_owned(), field.name.clone()));
                }
                Color::Black => {}
            }
        }
        colors.insert(node.to_owned(), Color::Black);
    }

    let mut colors = BTreeMap::new();
    let mut boxed = BTreeSet::new();
    for name in complex {
        if colors.get(name).copied().unwrap_or(Color::White) == Color::White {
            dfs(schema, name, complex, &mut colors, &mut boxed);
        }
    }
    boxed
}

fn render_types(
    schema: &Schema,
    complex: &BTreeSet<String>,
    enums: &BTreeSet<String>,
    namer: &Namer,
    boxed: &BTreeSet<(String, String)>,
) -> String {
    let mut out = String::new();
    out.push_str("// Generated from the public UCI 2.6 schema. Distribution A. Not operational.\n");
    out.push_str("#![allow(clippy::struct_field_names)]\n\n");
    out.push_str("use serde::{Deserialize, Serialize};\n\n");

    for name in enums {
        out.push_str(&render_enum(schema, name, namer));
        out.push('\n');
    }

    for name in complex {
        out.push_str(&render_struct(schema, name, namer, boxed));
        out.push('\n');
    }
    out
}

fn render_enum(schema: &Schema, name: &str, namer: &Namer) -> String {
    let rust_name = namer.type_name(name);
    let values = schema.enum_values(name);
    let mut out = String::new();
    out.push_str(&format!(
        "/// Values for `{name}`. Unknown values are preserved.\n"
    ));
    out.push_str("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n");
    out.push_str(&format!("pub enum {rust_name} {{\n"));
    let mut used = BTreeSet::new();
    let mut variants = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let mut variant = sanitize_variant(value);
        if variant.is_empty() || variant == "Unknown" {
            variant = format!("V{index}");
        }
        let base = variant.clone();
        let mut suffix = 2u32;
        while !used.insert(variant.clone()) {
            variant = format!("{base}_{suffix}");
            suffix += 1;
        }
        variants.push((variant, value.clone()));
    }
    for (variant, _) in &variants {
        out.push_str(&format!("    {variant},\n"));
    }
    out.push_str("    Unknown(String),\n");
    out.push_str("}\n\n");

    out.push_str(&format!(
        "impl Serialize for {rust_name} {{\n    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>\n    where S: serde::Serializer {{\n        match self {{\n"
    ));
    for (variant, value) in &variants {
        out.push_str(&format!(
            "            {rust_name}::{variant} => serializer.serialize_str({value:?}),\n"
        ));
    }
    out.push_str(&format!(
        "            {rust_name}::Unknown(value) => serializer.serialize_str(value),\n        }}\n    }}\n}}\n\n"
    ));
    out.push_str(&format!(
        "impl<'de> Deserialize<'de> for {rust_name} {{\n    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>\n    where D: serde::Deserializer<'de> {{\n        let value = <String as serde::Deserialize>::deserialize(deserializer)?;\n        Ok(match value.as_str() {{\n"
    ));
    for (variant, value) in &variants {
        out.push_str(&format!(
            "            {value:?} => {rust_name}::{variant},\n"
        ));
    }
    out.push_str(&format!(
        "            _ => {rust_name}::Unknown(value),\n        }})\n    }}\n}}\n"
    ));
    out
}

fn render_struct(
    schema: &Schema,
    type_name: &str,
    namer: &Namer,
    boxed: &BTreeSet<(String, String)>,
) -> String {
    let rust_name = namer.type_name(type_name);
    let docs = schema
        .complex(type_name)
        .map(|c| c.docs.clone())
        .unwrap_or_default();
    let mut out = String::new();
    for line in doc_lines(&docs) {
        out.push_str(&format!("/// {line}\n"));
    }
    out.push_str("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n");
    out.push_str(&format!("pub struct {rust_name} {{\n"));

    let choice_members: BTreeSet<String> = schema
        .choices(type_name)
        .into_iter()
        .flat_map(|c| c.members)
        .collect();

    let mut used_fields = BTreeSet::new();
    for field in schema.fields(type_name) {
        let mut field_name = sanitize_field(&field.name);
        let base = field_name.clone();
        let mut suffix = 2u32;
        while !used_fields.insert(field_name.clone()) {
            field_name = format!("{base}_{suffix}");
            suffix += 1;
        }
        let rust_type = rust_type(schema, namer, &field.type_name);
        let is_boxed = boxed.contains(&(type_name.to_owned(), field.name.clone()));
        let base_type = if is_boxed {
            format!("Box<{rust_type}>")
        } else {
            rust_type
        };
        let optional = field.min_occurs == 0 || choice_members.contains(&field.name);
        let repeated = field.max_occurs.is_repeated();
        let ty = match (optional, repeated) {
            (true, true) => format!("Option<Vec<{base_type}>>"),
            (false, true) => format!("Vec<{base_type}>"),
            (true, false) => format!("Option<{base_type}>"),
            (false, false) => base_type,
        };
        for line in doc_lines(&field.docs) {
            out.push_str(&format!("    /// {line}\n"));
        }
        out.push_str(&format!("    #[serde(rename = {:?})]\n", field.name));
        if optional {
            out.push_str("    #[serde(skip_serializing_if = \"Option::is_none\")]\n");
        }
        out.push_str(&format!("    pub {field_name}: {ty},\n"));
    }
    out.push_str("}\n");
    out
}

fn rust_type(schema: &Schema, namer: &Namer, name: &str) -> String {
    if let Some(TypeInfo::Complex(c)) = schema.types.get(name) {
        if c.abstract_ {
            return "serde_json::Value".to_owned();
        }
        return namer.type_name(name);
    }
    if let Some(TypeInfo::Simple(s)) = schema.types.get(name) {
        if !schema.enum_values(name).is_empty() {
            return namer.type_name(name);
        }
        return primitive(&schema.simple_builtin(&s.name));
    }
    primitive(name)
}

fn primitive(name: &str) -> String {
    match name {
        "boolean" => "bool".to_owned(),
        "int" | "integer" | "long" | "short" | "byte" | "nonPositiveInteger"
        | "negativeInteger" => "i64".to_owned(),
        "unsignedInt" | "unsignedLong" | "unsignedShort" | "unsignedByte" | "positiveInteger"
        | "nonNegativeInteger" => "u64".to_owned(),
        "double" | "float" | "decimal" => "f64".to_owned(),
        "dateTime" | "date" | "time" => "crate::DateTime".to_owned(),
        "hexBinary" => "crate::HexBytes".to_owned(),
        _ => "String".to_owned(),
    }
}

fn sanitize_type(name: &str) -> String {
    let trimmed = if let Some(base) = name.strip_suffix("MDT") {
        format!("{base}Data")
    } else if let Some(base) = name.strip_suffix("MT") {
        base.to_owned()
    } else {
        name.to_owned()
    };
    let mut out = trimmed.to_pascal_case();
    if out.is_empty() {
        out = "Anonymous".to_owned();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'T');
    }
    out
}

fn sanitize_variant(value: &str) -> String {
    let mut out = value.to_pascal_case();
    if out.is_empty() {
        out = "Empty".to_owned();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    out
}

fn sanitize_field(name: &str) -> String {
    let mut out = name.to_snake_case();
    if out.is_empty() {
        out = "field".to_owned();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if is_keyword(&out) {
        out.push('_');
    }
    out
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}

fn doc_lines(docs: &str) -> Vec<String> {
    docs.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.replace("*/", "* /"))
        .collect()
}

fn generated_tests(include_task: bool) -> String {
    let mut out = String::new();
    out.push_str("// Generated round-trip checks.\n\n");
    out.push_str("#[test]\nfn round_trips_primitives() {\n");
    out.push_str("    for _ in 0..100 {\n");
    out.push_str("        let time = uci_types::DateTime(\"2026-11-02T12:00:00Z\".to_owned());\n");
    out.push_str("        let text = serde_json::to_string(&time).unwrap();\n");
    out.push_str(
        "        let again: uci_types::DateTime = serde_json::from_str(&text).unwrap();\n",
    );
    out.push_str("        assert_eq!(time, again);\n");
    out.push_str("        let bytes = uci_types::HexBytes(vec![0, 15, 255]);\n");
    out.push_str("        let text = serde_json::to_string(&bytes).unwrap();\n");
    out.push_str(
        "        let again: uci_types::HexBytes = serde_json::from_str(&text).unwrap();\n",
    );
    out.push_str("        assert_eq!(bytes, again);\n");
    out.push_str("    }\n}\n");
    if include_task {
        out.push_str("\n#[test]\nfn round_trips_task_command() {\n");
        out.push_str(
            "    let text = include_str!(\"../../../../fixtures/messages/task_command.json\");\n",
        );
        out.push_str("    for _ in 0..100 {\n");
        out.push_str(
            "        let value: serde_json::Value = serde_json::from_str(text).unwrap();\n",
        );
        out.push_str("        let inner = &value[\"TaskCommand\"];\n");
        out.push_str("        let typed: uci_types::TaskCommand = serde_json::from_value(inner.clone()).unwrap();\n");
        out.push_str("        let encoded = serde_json::to_value(&typed).unwrap();\n");
        out.push_str("        let again: uci_types::TaskCommand = serde_json::from_value(encoded).unwrap();\n");
        out.push_str("        assert_eq!(typed, again);\n");
        out.push_str("    }\n}\n");
    }
    out
}

fn generated_manifest() -> String {
    "[package]\nname = \"uci-types\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"Apache-2.0\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n".to_owned()
}

fn generated_lib() -> String {
    "// Generated from the public UCI 2.6 schema. Distribution A. Not operational.\n\
     //! Typed bindings generated by `uci-forge`.\n\
     \n\
     mod types;\n\
     pub use types::*;\n\
     \n\
     /// An ISO 8601 timestamp preserved as text.\n\
     #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n\
     #[serde(transparent)]\n\
     pub struct DateTime(pub String);\n\
     \n\
     /// Hex-encoded binary data.\n\
     #[derive(Debug, Clone, PartialEq, Eq, Hash)]\n\
     pub struct HexBytes(pub Vec<u8>);\n\
     \n\
     impl serde::Serialize for HexBytes {\n\
         fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>\n\
         where S: serde::Serializer {\n\
             let mut text = String::with_capacity(self.0.len() * 2);\n\
             for byte in &self.0 {\n\
                 use std::fmt::Write as _;\n\
                 let _ = write!(text, \"{byte:02x}\");\n\
             }\n\
             serializer.serialize_str(&text)\n\
         }\n\
     }\n\
     \n\
     impl<'de> serde::Deserialize<'de> for HexBytes {\n\
         fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>\n\
         where D: serde::Deserializer<'de> {\n\
             let text = <String as serde::Deserialize>::deserialize(deserializer)?;\n\
             if text.len() % 2 != 0 {\n\
                 return Err(serde::de::Error::custom(\"odd-length hex string\"));\n\
             }\n\
             let mut bytes = Vec::with_capacity(text.len() / 2);\n\
             for chunk in text.as_bytes().chunks(2) {\n\
                 let pair = std::str::from_utf8(chunk)\n\
                     .map_err(|_| serde::de::Error::custom(\"invalid hex string\"))?;\n\
                 let byte = u8::from_str_radix(pair, 16)\n\
                     .map_err(|_| serde::de::Error::custom(\"invalid hex string\"))?;\n\
                 bytes.push(byte);\n\
             }\n\
             Ok(Self(bytes))\n\
         }\n\
     }\n"
    .to_owned()
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
    fn generates_mini_crate() {
        let schema = mini();
        let files = generate(&schema, &["MiniMessage".to_owned()]).expect("generate");
        assert_eq!(files.len(), 4);
        let types = files
            .iter()
            .find(|f| f.path == "src/types.rs")
            .expect("types file");
        assert!(types.contents.contains("pub struct MiniMessage"));
        assert!(types.contents.contains("pub struct TrackPayloadType"));
        assert!(types.contents.contains("pub enum ModeEnum"));
        assert!(types.contents.contains("Unknown(String)"));
        assert!(types.contents.contains("serde_json::Value"));
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(sanitize_type("TaskCommandMT"), "TaskCommand");
        assert_eq!(sanitize_type("TaskCommandMDT"), "TaskCommandData");
        assert_eq!(sanitize_type("AMTI_Capability"), "AmtiCapability");
        assert_eq!(sanitize_field("MessageID"), "message_id");
        assert_eq!(sanitize_field("type"), "type_");
        assert_eq!(sanitize_variant("AUTO"), "Auto");
        assert_eq!(sanitize_variant("1"), "V1");
    }

    #[test]
    fn rejects_unknown_message() {
        let schema = mini();
        assert_eq!(
            generate(&schema, &["Nope".to_owned()]),
            Err(CodegenError::UnknownMessage("Nope".to_owned()))
        );
    }

    #[test]
    fn rejects_unknown_field_type() {
        use uci_schema::{Content, TypeInfo};
        let mut schema = mini();
        let complex = schema
            .types
            .get_mut("MiniMessageMT")
            .and_then(|t| match t {
                TypeInfo::Complex(c) => Some(c),
                TypeInfo::Simple(_) => None,
            })
            .unwrap();
        let Content::Sequence { elements } = &mut complex.content else {
            panic!("MiniMessageMT is not a sequence");
        };
        let field = elements.iter_mut().find(|f| f.name == "Callsign").unwrap();
        field.type_name = "Mystery".to_owned();
        assert_eq!(
            generate(&schema, &["MiniMessage".to_owned()]),
            Err(CodegenError::UnknownType {
                owner: "MiniMessageMT".to_owned(),
                field: "Callsign".to_owned(),
                type_name: "Mystery".to_owned(),
            })
        );
    }

    #[test]
    fn builtins_are_recognized() {
        assert!(is_builtin("string"));
        assert!(is_builtin("dateTime"));
        assert!(is_builtin("hexBinary"));
        assert!(!is_builtin("CustomType"));
    }

    #[test]
    fn generates_default_suite_from_real_schema() {
        let spec = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/002.6.0");
        let schema = Schema::load_dir(&spec).expect("load 2.6");
        let names: Vec<String> = DEFAULT_MESSAGES.iter().map(|s| (*s).to_owned()).collect();
        let files = generate(&schema, &names).expect("generate");
        assert_eq!(files.len(), 4);
        let types = files
            .iter()
            .find(|f| f.path == "src/types.rs")
            .expect("types");
        assert!(types.contents.contains("pub struct TaskCommand"));
        assert!(types.contents.contains("pub enum"));
        assert!(
            types.contents.len() > 1_000_000,
            "generated output is smaller than expected"
        );
    }

    #[test]
    fn boxes_recursive_fields() {
        let schema = mini();
        let complex: BTreeSet<String> = ["PayloadType", "TrackPayloadType"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let boxed = boxed_edges(&schema, &complex);
        assert!(boxed.is_empty(), "mini schema has no recursion");
    }

    #[test]
    fn writes_files() {
        let dir = std::env::temp_dir().join(format!("uci-codegen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = generate(&mini(), &["MiniMessage".to_owned()]).expect("generate");
        write_files(&dir, &files).expect("write");
        assert!(dir.join("src/types.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_selection_and_bad_output() {
        assert_eq!(generate(&mini(), &[]), Err(CodegenError::NoMessages));
        let dir = std::env::temp_dir().join(format!("uci-codegen-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "x").unwrap();
        let file = GeneratedFile {
            path: "x/y".to_owned(),
            contents: "z".to_owned(),
        };
        assert!(write_files(&blocked, &[file]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_collide_and_resolve() {
        let mut complex = BTreeSet::new();
        complex.insert("FooMT".to_owned());
        complex.insert("Foo".to_owned());
        let namer = Namer::build(&complex, &BTreeSet::new());
        assert_ne!(namer.type_name("FooMT"), namer.type_name("Foo"));
        assert_eq!(sanitize_type(""), "Anonymous");
        assert_eq!(sanitize_type("1Foo"), "T1foo");
        assert_eq!(sanitize_field(""), "field");
        assert_eq!(sanitize_field("1x"), "_1x");
        assert_eq!(sanitize_variant(""), "Empty");
        assert_eq!(sanitize_variant("1"), "V1");
        assert_eq!(sanitize_variant("Unknown"), "Unknown");
    }

    #[test]
    fn boxes_mutual_recursion() {
        use uci_schema::{ComplexType, Content, Element, MaxOccurs, TypeInfo};
        fn ty(name: &str, field: &str) -> TypeInfo {
            TypeInfo::Complex(ComplexType {
                name: name.to_owned(),
                abstract_: false,
                docs: String::new(),
                base: None,
                content: Content::Sequence {
                    elements: vec![Element {
                        name: field.to_owned(),
                        type_name: if name == "A" { "B" } else { "A" }.to_owned(),
                        min_occurs: 1,
                        max_occurs: MaxOccurs::One,
                        docs: String::new(),
                    }],
                },
                subtypes: Vec::new(),
            })
        }
        let mut schema = Schema::default();
        schema.types.insert("A".to_owned(), ty("A", "b"));
        schema.types.insert("B".to_owned(), ty("B", "a"));
        let complex: BTreeSet<String> = ["A".to_owned(), "B".to_owned()].into_iter().collect();
        let boxed = boxed_edges(&schema, &complex);
        assert_eq!(boxed.len(), 1, "{boxed:?}");
    }

    #[test]
    fn renders_enum_edge_variants() {
        use uci_schema::{SimpleType, TypeInfo};
        let mut schema = Schema::default();
        schema.types.insert(
            "Weird".to_owned(),
            TypeInfo::Simple(SimpleType {
                name: "Weird".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: vec![
                    "".to_owned(),
                    "Unknown".to_owned(),
                    "A".to_owned(),
                    "A".to_owned(),
                    "1".to_owned(),
                ],
                facets: Vec::new(),
            }),
        );
        let mut enums = BTreeSet::new();
        enums.insert("Weird".to_owned());
        let namer = Namer::build(&BTreeSet::new(), &enums);
        let out = render_enum(&schema, "Weird", &namer);
        assert!(out.contains("V1"));
        assert!(out.contains("A_2"));
        assert!(out.contains("Unknown(String)"));
    }

    #[test]
    fn primitives_cover_builtins() {
        assert_eq!(primitive("boolean"), "bool");
        assert_eq!(primitive("int"), "i64");
        assert_eq!(primitive("unsignedInt"), "u64");
        assert_eq!(primitive("double"), "f64");
        assert_eq!(primitive("dateTime"), "crate::DateTime");
        assert_eq!(primitive("hexBinary"), "crate::HexBytes");
        assert_eq!(primitive("string"), "String");
        assert_eq!(primitive("weird"), "String");
    }

    #[test]
    fn doc_lines_scrub_terminators() {
        assert_eq!(doc_lines("a\n\nb\n"), vec!["a", "b"]);
        assert_eq!(doc_lines("/* x */"), vec!["/* x * /"]);
    }

    #[test]
    fn field_collisions_get_suffixes() {
        use uci_schema::{ComplexType, Content, Element, MaxOccurs, TypeInfo};
        let mut schema = Schema::default();
        schema.types.insert(
            "Thing".to_owned(),
            TypeInfo::Complex(ComplexType {
                name: "Thing".to_owned(),
                abstract_: false,
                docs: String::new(),
                base: None,
                content: Content::Sequence {
                    elements: vec![
                        Element {
                            name: "Foo".to_owned(),
                            type_name: "string".to_owned(),
                            min_occurs: 0,
                            max_occurs: MaxOccurs::One,
                            docs: String::new(),
                        },
                        Element {
                            name: "foo".to_owned(),
                            type_name: "string".to_owned(),
                            min_occurs: 0,
                            max_occurs: MaxOccurs::One,
                            docs: String::new(),
                        },
                    ],
                },
                subtypes: Vec::new(),
            }),
        );
        let mut complex = BTreeSet::new();
        complex.insert("Thing".to_owned());
        let namer = Namer::build(&complex, &BTreeSet::new());
        let out = render_struct(&schema, "Thing", &namer, &BTreeSet::new());
        assert!(out.contains("foo_2"), "{out}");
    }
}
