//! Parse the public UCI XSD into a stable model.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Errors produced while loading or parsing a schema.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// Filesystem error.
    #[error("io error at {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// XML syntax error.
    #[error("xml error in {path}: {message}")]
    Xml {
        /// Path that failed.
        path: PathBuf,
        /// Parser message.
        message: String,
    },
    /// The schema is structurally unusable.
    #[error("schema error: {0}")]
    Invalid(String),
}

/// A parsed UCI schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    /// Schema version, for example `002.6.0`.
    pub version: String,
    /// Message roots, keyed by element name.
    pub messages: BTreeMap<String, MessageInfo>,
    /// All global element roots, keyed by element name.
    pub roots: BTreeMap<String, String>,
    /// Named types.
    pub types: BTreeMap<String, TypeInfo>,
}

/// A UCI message root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    /// Element name.
    pub name: String,
    /// UCI primitive tag, when present.
    pub primitive: Option<String>,
    /// One-line summary.
    pub summary: String,
    /// Full documentation.
    pub docs: String,
    /// Named root type.
    pub root_type: String,
}

/// A named schema type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TypeInfo {
    /// Complex type with fields.
    Complex(ComplexType),
    /// Simple type with a base and optional facets.
    Simple(SimpleType),
}

/// A complex type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexType {
    /// Type name.
    pub name: String,
    /// Whether the type is abstract.
    pub abstract_: bool,
    /// Documentation text.
    pub docs: String,
    /// Base type name, when the type extends another.
    pub base: Option<String>,
    /// Own content.
    pub content: Content,
    /// Direct and indirect subtypes, computed at load time.
    pub subtypes: Vec<String>,
}

/// Content model of a complex type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Content {
    /// No fields.
    Empty,
    /// Ordered sequence of fields.
    Sequence {
        /// Fields.
        elements: Vec<Element>,
    },
    /// Choice group.
    Choice {
        /// Fields.
        elements: Vec<Element>,
        /// Minimum members present.
        min_occurs: u32,
        /// Maximum members present.
        max_occurs: MaxOccurs,
    },
}

/// Field cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MaxOccurs {
    /// Exactly one.
    One,
    /// Bounded above one.
    Bounded {
        /// Upper bound.
        n: u32,
    },
    /// No upper bound.
    Unbounded,
}

impl MaxOccurs {
    /// Whether the field is repeated.
    #[must_use]
    pub fn is_repeated(self) -> bool {
        !matches!(self, Self::One)
    }
}

/// A choice group constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceGroup {
    /// Member field names.
    pub members: Vec<String>,
    /// Minimum members present.
    pub min_occurs: u32,
    /// Maximum members present.
    pub max_occurs: MaxOccurs,
}

/// A field in a complex type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    /// JSON key.
    pub name: String,
    /// Referenced type name.
    pub type_name: String,
    /// Minimum occurrences.
    pub min_occurs: u32,
    /// Maximum occurrences.
    pub max_occurs: MaxOccurs,
    /// Documentation text.
    pub docs: String,
}

/// A simple type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleType {
    /// Type name.
    pub name: String,
    /// Direct base name.
    pub base: String,
    /// Documentation text.
    pub docs: String,
    /// Enumeration values, in declaration order.
    pub enum_values: Vec<String>,
    /// Facets as `(kind, value)` pairs.
    pub facets: Vec<Facet>,
}

/// An XSD facet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facet {
    /// Facet kind, for example `pattern`.
    pub kind: String,
    /// Facet value.
    pub value: String,
}

impl Schema {
    /// Load every `.xsd` file in a directory, sorted by path.
    pub fn load_dir(dir: &Path) -> Result<Self, SchemaError> {
        let mut paths = Vec::new();
        collect_xsd(dir, &mut paths)?;
        paths.sort();
        if paths.is_empty() {
            return Err(SchemaError::Invalid(format!(
                "no .xsd files under {}",
                dir.display()
            )));
        }
        Self::load_files(&paths)
    }

    /// Load an explicit list of XSD files.
    pub fn load_files(paths: &[PathBuf]) -> Result<Self, SchemaError> {
        let mut schema = Self::default();
        for path in paths {
            parse_file(path, &mut schema)?;
        }
        schema.finalize();
        Ok(schema)
    }

    /// Number of message roots.
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Number of complex types.
    #[must_use]
    pub fn complex_count(&self) -> usize {
        self.types
            .values()
            .filter(|t| matches!(t, TypeInfo::Complex(_)))
            .count()
    }

    /// Number of simple types.
    #[must_use]
    pub fn simple_count(&self) -> usize {
        self.types
            .values()
            .filter(|t| matches!(t, TypeInfo::Simple(_)))
            .count()
    }

    /// Total enumeration values.
    #[must_use]
    pub fn enum_value_count(&self) -> usize {
        self.types
            .values()
            .filter_map(|t| match t {
                TypeInfo::Simple(s) => Some(s.enum_values.len()),
                TypeInfo::Complex(_) => None,
            })
            .sum()
    }

    /// Look up a complex type.
    #[must_use]
    pub fn complex(&self, name: &str) -> Option<&ComplexType> {
        match self.types.get(name) {
            Some(TypeInfo::Complex(c)) => Some(c),
            _ => None,
        }
    }

    /// Look up a simple type.
    #[must_use]
    pub fn simple(&self, name: &str) -> Option<&SimpleType> {
        match self.types.get(name) {
            Some(TypeInfo::Simple(s)) => Some(s),
            _ => None,
        }
    }

    /// Whether a type is abstract.
    #[must_use]
    pub fn is_abstract(&self, name: &str) -> bool {
        self.complex(name).is_some_and(|c| c.abstract_)
    }

    /// Direct and indirect subtypes of a complex type.
    #[must_use]
    pub fn subtypes(&self, name: &str) -> &[String] {
        self.complex(name).map_or(&[], |c| c.subtypes.as_slice())
    }

    /// Resolve a simple type to its XSD builtin name, for example `string`.
    #[must_use]
    pub fn simple_builtin(&self, name: &str) -> String {
        let mut current = name.to_owned();
        let mut guard = 0usize;
        while guard < 32 {
            guard += 1;
            let Some(simple) = self.simple(&current) else {
                break;
            };
            if simple.base == current {
                break;
            }
            current = simple.base.clone();
        }
        current
    }

    /// Enumeration values for a type, following the base chain.
    #[must_use]
    pub fn enum_values(&self, name: &str) -> Vec<String> {
        let mut current = name.to_owned();
        let mut guard = 0usize;
        while guard < 32 {
            guard += 1;
            let Some(simple) = self.simple(&current) else {
                break;
            };
            if !simple.enum_values.is_empty() {
                return simple.enum_values.clone();
            }
            if simple.base == current {
                break;
            }
            current = simple.base.clone();
        }
        Vec::new()
    }

    /// Flattened fields for a complex type, base fields first.
    #[must_use]
    pub fn fields(&self, type_name: &str) -> Vec<Element> {
        let mut chain = Vec::new();
        let mut current = Some(type_name.to_owned());
        let mut guard = 0usize;
        while let Some(name) = current {
            guard += 1;
            if guard > 64 {
                break;
            }
            let Some(complex) = self.complex(&name) else {
                break;
            };
            chain.push(complex.clone());
            current = complex.base.clone();
        }
        chain.reverse();

        let mut out: Vec<Element> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for complex in chain {
            let elements = match complex.content {
                Content::Empty => Vec::new(),
                Content::Sequence { elements } | Content::Choice { elements, .. } => elements,
            };
            for element in elements {
                if seen.insert(element.name.clone()) {
                    out.push(element);
                }
            }
        }
        out
    }

    /// The choice group for a type, if its own content is a choice.
    #[must_use]
    pub fn choice(&self, type_name: &str) -> Option<(Vec<Element>, u32, MaxOccurs)> {
        let complex = self.complex(type_name)?;
        match &complex.content {
            Content::Choice {
                elements,
                min_occurs,
                max_occurs,
            } => Some((elements.clone(), *min_occurs, *max_occurs)),
            _ => None,
        }
    }

    /// All choice groups in a type's inheritance chain.
    #[must_use]
    pub fn choices(&self, type_name: &str) -> Vec<ChoiceGroup> {
        let mut out = Vec::new();
        let mut current = Some(type_name.to_owned());
        let mut guard = 0usize;
        while let Some(name) = current {
            guard += 1;
            if guard > 64 {
                break;
            }
            let Some(complex) = self.complex(&name) else {
                break;
            };
            if let Content::Choice {
                elements,
                min_occurs,
                max_occurs,
            } = &complex.content
            {
                out.push(ChoiceGroup {
                    members: elements.iter().map(|e| e.name.clone()).collect(),
                    min_occurs: *min_occurs,
                    max_occurs: *max_occurs,
                });
            }
            current = complex.base.clone();
        }
        out
    }

    /// Every complex type reachable from a root type, including the root.
    #[must_use]
    pub fn reachable_types(&self, roots: &[String]) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<String> = roots.to_vec();
        while let Some(name) = stack.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(complex) = self.complex(&name) else {
                continue;
            };
            let elements = match &complex.content {
                Content::Empty => Vec::new(),
                Content::Sequence { elements } | Content::Choice { elements, .. } => {
                    elements.clone()
                }
            };
            for element in elements {
                if self.complex(&element.type_name).is_some() {
                    stack.push(element.type_name);
                }
            }
            if let Some(base) = &complex.base
                && self.complex(base).is_some()
            {
                stack.push(base.clone());
            }
        }
        seen
    }

    /// Parse a single XSD document from memory. Includes are not resolved.
    pub fn parse_xsd(text: &str) -> Result<Self, SchemaError> {
        let mut schema = Self::default();
        parse_text(Path::new("<memory>"), text, &mut schema)?;
        schema.finalize();
        Ok(schema)
    }

    /// Export deterministically as pretty JSON.
    pub fn to_json(&self) -> Result<String, SchemaError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| SchemaError::Invalid(format!("serialize schema: {e}")))
    }

    fn finalize(&mut self) {
        // Resolve subtypes before flattening, so codegen can consult them.
        let mut edges: Vec<(String, String)> = Vec::new();
        for (name, info) in &self.types {
            if let TypeInfo::Complex(c) = info
                && let Some(base) = &c.base
                && self.types.contains_key(base)
            {
                edges.push((base.clone(), name.clone()));
            }
        }
        for (base, child) in edges {
            if let Some(TypeInfo::Complex(c)) = self.types.get_mut(&base)
                && !c.subtypes.contains(&child)
            {
                c.subtypes.push(child);
            }
        }
        for info in self.types.values_mut() {
            if let TypeInfo::Complex(c) = info {
                c.subtypes.sort();
            }
        }

        // Promote message roots.
        let roots: Vec<(String, String)> = self
            .roots
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (name, type_name) in roots {
            if !type_name.ends_with("MT") {
                continue;
            }
            let (docs, primitive) = match self.complex(&type_name) {
                Some(c) => (
                    c.docs.clone(),
                    extract_primitive(&c.docs).or_else(|| extract_primitive("")),
                ),
                None => (String::new(), None),
            };
            let summary = summarize(&docs);
            self.messages.insert(
                name.clone(),
                MessageInfo {
                    name,
                    primitive,
                    summary,
                    docs,
                    root_type: type_name,
                },
            );
        }
    }
}

fn collect_xsd(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), SchemaError> {
    let entries = std::fs::read_dir(dir).map_err(|source| SchemaError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| SchemaError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_xsd(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "xsd") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_file(path: &Path, schema: &mut Schema) -> Result<(), SchemaError> {
    let text = std::fs::read_to_string(path).map_err(|source| SchemaError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_text(path, &text, schema)
}

fn parse_text(path: &Path, text: &str, schema: &mut Schema) -> Result<(), SchemaError> {
    let doc = roxmltree::Document::parse(text).map_err(|e| SchemaError::Xml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let root = doc.root_element();
    if root.tag_name().name() != "schema" {
        return Err(SchemaError::Invalid(format!(
            "{}: root element is not xs:schema",
            path.display()
        )));
    }
    if schema.version.is_empty()
        && let Some(version) = root.attribute("version")
    {
        schema.version = version.to_owned();
    }

    for node in root.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "element" => {
                let Some(name) = node.attribute("name") else {
                    continue;
                };
                let type_name = node
                    .attribute("type")
                    .map(strip_prefix)
                    .or_else(|| node.attribute("ref").map(strip_prefix))
                    .unwrap_or_else(|| "string".to_owned());
                schema.roots.insert(name.to_owned(), type_name);
            }
            "complexType" => {
                let Some(name) = node.attribute("name") else {
                    continue;
                };
                let complex = parse_complex(node, name);
                schema
                    .types
                    .insert(name.to_owned(), TypeInfo::Complex(complex));
            }
            "simpleType" => {
                let Some(name) = node.attribute("name") else {
                    continue;
                };
                let simple = parse_simple(node, name);
                schema
                    .types
                    .insert(name.to_owned(), TypeInfo::Simple(simple));
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_complex(node: roxmltree::Node<'_, '_>, name: &str) -> ComplexType {
    let docs = docs_of(node);
    let abstract_ = node.attribute("abstract") == Some("true");
    let (base, content) = if let Some(complex_content) = child(node, "complexContent")
        && let Some(extension) = child(complex_content, "extension")
    {
        (
            extension.attribute("base").map(strip_prefix),
            parse_content(extension),
        )
    } else {
        (None, parse_content(node))
    };

    ComplexType {
        name: name.to_owned(),
        abstract_,
        docs,
        base,
        content,
        subtypes: Vec::new(),
    }
}

fn parse_content(node: roxmltree::Node<'_, '_>) -> Content {
    if let Some(sequence) = child(node, "sequence") {
        return Content::Sequence {
            elements: parse_elements(sequence),
        };
    }
    if let Some(choice) = child(node, "choice") {
        return Content::Choice {
            elements: parse_elements(choice),
            min_occurs: parse_u32(choice.attribute("minOccurs"), 1),
            max_occurs: parse_max(choice.attribute("maxOccurs")),
        };
    }
    Content::Empty
}

fn parse_elements(parent: roxmltree::Node<'_, '_>) -> Vec<Element> {
    parent
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "element")
        .filter_map(|n| {
            let name = n
                .attribute("name")
                .or_else(|| n.attribute("ref"))
                .map(strip_prefix)?;
            let type_name = n
                .attribute("type")
                .map(strip_prefix)
                .unwrap_or_else(|| "string".to_owned());
            Some(Element {
                name,
                type_name,
                min_occurs: parse_u32(n.attribute("minOccurs"), 1),
                max_occurs: parse_max(n.attribute("maxOccurs")),
                docs: docs_of(n),
            })
        })
        .collect()
}

fn parse_simple(node: roxmltree::Node<'_, '_>, name: &str) -> SimpleType {
    let docs = docs_of(node);
    let mut base = "string".to_owned();
    let mut enum_values = Vec::new();
    let mut facets = Vec::new();
    if let Some(restriction) = child(node, "restriction") {
        if let Some(value) = restriction.attribute("base") {
            base = strip_prefix(value);
        }
        for facet in restriction.children().filter(|n| n.is_element()) {
            let kind = facet.tag_name().name();
            let value = facet.attribute("value").unwrap_or_default().to_owned();
            if kind == "enumeration" {
                enum_values.push(value);
            } else {
                facets.push(Facet {
                    kind: kind.to_owned(),
                    value,
                });
            }
        }
    }
    SimpleType {
        name: name.to_owned(),
        base,
        docs,
        enum_values,
        facets,
    }
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

fn docs_of(node: roxmltree::Node<'_, '_>) -> String {
    let mut parts = Vec::new();
    for annotation in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "annotation")
    {
        for doc in annotation
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "documentation")
        {
            let text = doc
                .descendants()
                .filter_map(|n| n.text())
                .collect::<Vec<_>>()
                .join(" ");
            let text = collapse(&text);
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summarize(docs: &str) -> String {
    docs.lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with("UCI_PRIMITIVE")
                && !line.starts_with("DISCLAIMER")
        })
        .map(|line| {
            if line.chars().count() > 180 {
                let mut out: String = line.chars().take(177).collect();
                out.push_str("...");
                out
            } else {
                line.to_owned()
            }
        })
        .unwrap_or_default()
}

fn extract_primitive(docs: &str) -> Option<String> {
    docs.lines().find_map(|line| {
        line.trim()
            .strip_prefix("UCI_PRIMITIVE:")
            .map(|rest| rest.trim().trim_end_matches('.').to_owned())
    })
}

fn strip_prefix(value: &str) -> String {
    value
        .rsplit_once(':')
        .map_or(value, |(_, local)| local)
        .to_owned()
}

fn parse_u32(value: Option<&str>, default: u32) -> u32 {
    value.and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_max(value: Option<&str>) -> MaxOccurs {
    match value {
        None | Some("1") => MaxOccurs::One,
        Some("unbounded") => MaxOccurs::Unbounded,
        Some(other) => other.parse::<u32>().map_or(MaxOccurs::One, |n| {
            if n <= 1 {
                MaxOccurs::One
            } else {
                MaxOccurs::Bounded { n }
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(version: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../spec")
            .join(version)
    }

    #[test]
    fn parses_26_real_schema() {
        let schema = Schema::load_dir(&spec("002.6.0")).expect("load 2.6");
        assert_eq!(schema.version, "002.6.0");
        assert_eq!(schema.message_count(), 725, "message count");
        assert_eq!(schema.complex_count(), 4636, "complex type count");
        assert_eq!(schema.simple_count(), 935, "simple type count");
        assert_eq!(schema.enum_value_count(), 7767, "enum value count");
        assert!(schema.messages.contains_key("TaskCommand"));
        assert!(schema.messages.contains_key("SignalReport"));
        assert!(schema.messages.contains_key("PositionReport"));
    }

    #[test]
    fn flattens_extension_chain() {
        let schema = Schema::load_dir(&spec("002.6.0")).expect("load 2.6");
        let fields = schema.fields("TaskCommandMT");
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"MessageData"), "{names:?}");
        assert!(names.contains(&"MessageHeader"), "{names:?}");
        assert!(names.contains(&"SecurityInformation"), "{names:?}");
    }

    #[test]
    fn resolves_enum_values() {
        let schema = Schema::load_dir(&spec("002.6.0")).expect("load 2.6");
        let values = schema.enum_values("EntityStatusEnum");
        assert!(!values.is_empty(), "EntityStatusEnum should have values");
    }

    #[test]
    fn exports_deterministic_json() {
        let schema = Schema::load_dir(&spec("002.6.0")).expect("load 2.6");
        let a = schema.to_json().expect("json a");
        let b = schema.to_json().expect("json b");
        assert_eq!(a, b);
        assert!(a.len() > 1_000_000, "export should include docs");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uci-schema-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_empty_directory() {
        let dir = temp_dir("empty");
        assert!(matches!(
            Schema::load_dir(&dir),
            Err(SchemaError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_directory() {
        let dir = temp_dir("missing");
        let missing = dir.join("nope");
        assert!(matches!(
            Schema::load_dir(&missing),
            Err(SchemaError::Io { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_malformed_xml() {
        let dir = temp_dir("xml");
        let path = dir.join("bad.xsd");
        std::fs::write(&path, "<not xml").unwrap();
        assert!(matches!(
            Schema::load_files(&[path]),
            Err(SchemaError::Xml { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_schema_root() {
        let dir = temp_dir("root");
        let path = dir.join("bad.xsd");
        std::fs::write(&path, "<foo/>").unwrap();
        assert!(matches!(
            Schema::load_files(&[path]),
            Err(SchemaError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_unnamed_declarations_and_nested_dirs() {
        let dir = temp_dir("nested");
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let path = nested.join("mini.xsd");
        std::fs::write(
            &path,
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" version="1.0">
            <xs:element/>
            <xs:complexType/>
            <xs:simpleType/>
            <xs:element name="Thing" type="uci:ThingMT"/>
            </xs:schema>"#,
        )
        .unwrap();
        let schema = Schema::load_dir(&dir).unwrap();
        assert_eq!(schema.version, "1.0");
        assert!(schema.messages.contains_key("Thing"));
        assert!(schema.messages["Thing"].docs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accessors_handle_missing_and_self_referential_types() {
        let mut schema = Schema::default();
        assert!(!schema.is_abstract("Nope"));
        assert!(schema.subtypes("Nope").is_empty());
        assert_eq!(schema.fields("Nope").len(), 0);
        assert!(schema.choice("Nope").is_none());
        assert!(schema.choices("Nope").is_empty());
        assert_eq!(schema.simple_builtin("Nope"), "Nope");
        assert!(schema.enum_values("Nope").is_empty());

        schema.types.insert(
            "Loop".to_owned(),
            TypeInfo::Simple(SimpleType {
                name: "Loop".to_owned(),
                base: "Loop".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: Vec::new(),
            }),
        );
        assert_eq!(schema.simple_builtin("Loop"), "Loop");
        assert!(schema.enum_values("Loop").is_empty());
    }

    #[test]
    fn mini_choice_and_abstract_accessors() {
        let schema =
            Schema::load_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"))
                .unwrap();
        assert!(schema.is_abstract("PayloadType"));
        assert!(schema.choice("ChoiceType").is_some());
        assert!(schema.choices("MiniMessageMT").is_empty());
        assert!(
            schema
                .subtypes("PayloadType")
                .contains(&"TrackPayloadType".to_owned())
        );
    }

    #[test]
    fn helpers_cover_edges() {
        assert_eq!(strip_prefix("uci:Foo"), "Foo");
        assert_eq!(strip_prefix("Foo"), "Foo");
        assert_eq!(
            extract_primitive("UCI_PRIMITIVE: Data-1."),
            Some("Data-1".to_owned())
        );
        assert_eq!(extract_primitive("none"), None);
        assert_eq!(collapse(" a  b "), "a b");
        assert_eq!(parse_u32(None, 3), 3);
        assert_eq!(parse_u32(Some("x"), 3), 3);
        assert_eq!(parse_u32(Some("7"), 3), 7);
        assert_eq!(parse_max(None), MaxOccurs::One);
        assert_eq!(parse_max(Some("1")), MaxOccurs::One);
        assert_eq!(parse_max(Some("0")), MaxOccurs::One);
        assert_eq!(parse_max(Some("2")), MaxOccurs::Bounded { n: 2 });
        assert_eq!(parse_max(Some("unbounded")), MaxOccurs::Unbounded);
        assert_eq!(parse_max(Some("x")), MaxOccurs::One);
    }

    #[test]
    fn parses_from_memory_and_rejects_garbage() {
        let mini = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mini.xsd"),
        )
        .unwrap();
        let schema = Schema::parse_xsd(&mini).unwrap();
        assert!(schema.messages.contains_key("MiniMessage"));
        assert!(Schema::parse_xsd("not xml").is_err());
        assert!(Schema::parse_xsd("<foo/>").is_err());
    }

    #[test]
    fn random_input_never_panics() {
        let mut state = 0x1234_5678_9abc_def0u64;
        for _ in 0..500 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let length = (state % 64) as usize;
            let mut text = String::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push(char::from((state % 128) as u8));
            }
            let _ = Schema::parse_xsd(&text);
        }
    }

    #[test]
    fn summarize_truncates_long_lines() {
        let long = "a".repeat(200);
        let summary = summarize(&long);
        assert_eq!(summary.chars().count(), 180);
        assert!(summary.ends_with("..."));
        assert_eq!(summarize(""), "");
        assert_eq!(
            summarize(
                "UCI_PRIMITIVE: X-1.
Real summary"
            ),
            "Real summary"
        );
    }
}
