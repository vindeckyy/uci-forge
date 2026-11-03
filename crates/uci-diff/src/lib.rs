//! Compare two UCI schema versions and classify wire-compatibility changes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uci_schema::{Element, MaxOccurs, Schema, TypeInfo};

/// Compatibility class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Safe for both producers and consumers.
    Compatible,
    /// Safe to produce, risky to consume exhaustively.
    Risky,
    /// Breaks at least one side.
    Breaking,
}

impl Class {
    /// Stable lowercase label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Risky => "risky",
            Self::Breaking => "breaking",
        }
    }
}

/// One schema change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Stable kind, for example `FIELD_ADDED`.
    pub kind: String,
    /// Compatibility class.
    pub class: Class,
    /// Message name, when the change belongs to one.
    pub message: Option<String>,
    /// Path or type name.
    pub path: String,
    /// Human detail.
    pub detail: String,
}

/// A full diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffReport {
    /// Old version.
    pub from: String,
    /// New version.
    pub to: String,
    /// Changes, sorted by class then path.
    pub changes: Vec<Change>,
}

impl DiffReport {
    /// Whether any breaking change exists.
    #[must_use]
    pub fn has_breaking(&self) -> bool {
        self.changes.iter().any(|c| c.class == Class::Breaking)
    }

    /// Count changes by class.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut compatible = 0;
        let mut risky = 0;
        let mut breaking = 0;
        for change in &self.changes {
            match change.class {
                Class::Compatible => compatible += 1,
                Class::Risky => risky += 1,
                Class::Breaking => breaking += 1,
            }
        }
        (compatible, risky, breaking)
    }

    /// Render a Markdown report.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let (compatible, risky, breaking) = self.counts();
        let mut out = String::new();
        out.push_str(&format!("# UCI {} to {}\n\n", self.from, self.to));
        out.push_str(&format!(
            "{} breaking, {} risky, {} compatible.\n\n",
            breaking, risky, compatible
        ));
        out.push_str("| Class | Kind | Message | Path | Detail |\n");
        out.push_str("|---|---|---|---|---|\n");
        for change in &self.changes {
            out.push_str(&format!(
                "| {} | {} | {} | `{}` | {} |\n",
                change.class.label(),
                change.kind,
                change.message.as_deref().unwrap_or(""),
                change.path,
                change.detail.replace('|', "\\|")
            ));
        }
        out
    }
}

/// Compare two schemas.
#[must_use]
pub fn diff(old: &Schema, new: &Schema) -> DiffReport {
    let mut changes = Vec::new();
    compare_messages(old, new, &mut changes);
    compare_enums(old, new, &mut changes);
    changes.sort_by(|a, b| {
        b.class
            .cmp(&a.class)
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| a.path.cmp(&b.path))
    });
    DiffReport {
        from: old.version.clone(),
        to: new.version.clone(),
        changes,
    }
}

fn compare_messages(old: &Schema, new: &Schema, out: &mut Vec<Change>) {
    for name in new.messages.keys() {
        if !old.messages.contains_key(name) {
            out.push(Change {
                kind: "MESSAGE_ADDED".to_owned(),
                class: Class::Compatible,
                message: Some(name.clone()),
                path: name.clone(),
                detail: "new message root".to_owned(),
            });
        }
    }
    for (name, old_info) in &old.messages {
        let Some(new_info) = new.messages.get(name) else {
            out.push(Change {
                kind: "MESSAGE_REMOVED".to_owned(),
                class: Class::Breaking,
                message: Some(name.clone()),
                path: name.clone(),
                detail: "message root removed".to_owned(),
            });
            continue;
        };
        if old_info.root_type != new_info.root_type {
            out.push(Change {
                kind: "MESSAGE_ROOT_CHANGED".to_owned(),
                class: Class::Breaking,
                message: Some(name.clone()),
                path: name.clone(),
                detail: format!("{} became {}", old_info.root_type, new_info.root_type),
            });
        }
        let old_fields = field_map(old, &old_info.root_type);
        let new_fields = field_map(new, &new_info.root_type);
        for (field_name, new_field) in &new_fields {
            if !old_fields.contains_key(field_name) {
                out.push(Change {
                    kind: "FIELD_ADDED".to_owned(),
                    class: if new_field.min_occurs > 0 {
                        Class::Breaking
                    } else {
                        Class::Compatible
                    },
                    message: Some(name.clone()),
                    path: format!("{name}.{field_name}"),
                    detail: if new_field.min_occurs > 0 {
                        "required field added".to_owned()
                    } else {
                        "optional field added".to_owned()
                    },
                });
            }
        }
        for (field_name, old_field) in &old_fields {
            let Some(new_field) = new_fields.get(field_name) else {
                out.push(Change {
                    kind: "FIELD_REMOVED".to_owned(),
                    class: Class::Breaking,
                    message: Some(name.clone()),
                    path: format!("{name}.{field_name}"),
                    detail: "field removed".to_owned(),
                });
                continue;
            };
            compare_field(name, field_name, old_field, new_field, out);
        }
    }
}

fn field_map(schema: &Schema, root_type: &str) -> BTreeMap<String, Element> {
    schema
        .fields(root_type)
        .into_iter()
        .map(|f| (f.name.clone(), f))
        .collect()
}

fn compare_field(
    message: &str,
    field_name: &str,
    old: &Element,
    new: &Element,
    out: &mut Vec<Change>,
) {
    let path = format!("{message}.{field_name}");
    if old.type_name != new.type_name {
        out.push(Change {
            kind: "FIELD_TYPE_CHANGED".to_owned(),
            class: Class::Breaking,
            message: Some(message.to_owned()),
            path: path.clone(),
            detail: format!("{} became {}", old.type_name, new.type_name),
        });
    }
    if old.min_occurs != new.min_occurs {
        out.push(Change {
            kind: "FIELD_OPTIONALITY_CHANGED".to_owned(),
            class: if new.min_occurs > old.min_occurs {
                Class::Breaking
            } else {
                Class::Compatible
            },
            message: Some(message.to_owned()),
            path: path.clone(),
            detail: format!("minOccurs {} became {}", old.min_occurs, new.min_occurs),
        });
    }
    if old.max_occurs != new.max_occurs {
        let class = if cardinality_narrowed(old.max_occurs, new.max_occurs) {
            Class::Breaking
        } else {
            Class::Compatible
        };
        out.push(Change {
            kind: "FIELD_CARDINALITY_CHANGED".to_owned(),
            class,
            message: Some(message.to_owned()),
            path,
            detail: format!("maxOccurs {:?} became {:?}", old.max_occurs, new.max_occurs),
        });
    }
}

fn cardinality_narrowed(old: MaxOccurs, new: MaxOccurs) -> bool {
    match (old, new) {
        (MaxOccurs::One, MaxOccurs::One) => false,
        (MaxOccurs::One, _) => false,
        (_, MaxOccurs::One) => true,
        (MaxOccurs::Unbounded, MaxOccurs::Bounded { .. }) => true,
        (MaxOccurs::Bounded { .. }, MaxOccurs::Unbounded) => false,
        (MaxOccurs::Bounded { n: old }, MaxOccurs::Bounded { n: new }) => new < old,
        (MaxOccurs::Unbounded, MaxOccurs::Unbounded) => false,
    }
}

fn compare_enums(old: &Schema, new: &Schema, out: &mut Vec<Change>) {
    for (name, old_type) in &old.types {
        let TypeInfo::Simple(old_simple) = old_type else {
            continue;
        };
        if old_simple.enum_values.is_empty() {
            continue;
        }
        let Some(TypeInfo::Simple(new_simple)) = new.types.get(name) else {
            continue;
        };
        for value in &new_simple.enum_values {
            if !old_simple.enum_values.contains(value) {
                out.push(Change {
                    kind: "ENUM_VALUE_ADDED".to_owned(),
                    class: Class::Risky,
                    message: None,
                    path: format!("type.{name}"),
                    detail: format!("value {value} added"),
                });
            }
        }
        for value in &old_simple.enum_values {
            if !new_simple.enum_values.contains(value) {
                out.push(Change {
                    kind: "ENUM_VALUE_REMOVED".to_owned(),
                    class: Class::Breaking,
                    message: None,
                    path: format!("type.{name}"),
                    detail: format!("value {value} removed"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uci_schema::{ComplexType, Content, SimpleType};

    fn mini() -> Schema {
        Schema::load_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
        )
        .expect("load mini schema")
    }

    fn fields_mut<'a>(schema: &'a mut Schema, type_name: &str) -> &'a mut Vec<Element> {
        let Some(TypeInfo::Complex(ComplexType {
            content: Content::Sequence { elements },
            ..
        })) = schema.types.get_mut(type_name)
        else {
            panic!("expected sequence type {type_name}");
        };
        elements
    }

    fn kinds(report: &DiffReport) -> Vec<&str> {
        report.changes.iter().map(|c| c.kind.as_str()).collect()
    }

    #[test]
    fn identical_schemas_have_no_changes() {
        let schema = mini();
        let report = diff(&schema, &schema);
        assert!(report.changes.is_empty());
    }

    #[test]
    fn detects_message_added_and_removed() {
        let old = mini();
        let mut new = mini();
        new.messages
            .insert("Other".to_owned(), old.messages["MiniMessage"].clone());
        let report = diff(&old, &new);
        assert!(kinds(&report).contains(&"MESSAGE_ADDED"));
        assert_eq!(
            report
                .changes
                .iter()
                .find(|c| c.kind == "MESSAGE_ADDED")
                .unwrap()
                .class,
            Class::Compatible
        );

        let report = diff(&new, &old);
        assert!(kinds(&report).contains(&"MESSAGE_REMOVED"));
        assert!(report.has_breaking());
    }

    #[test]
    fn detects_optional_field_added() {
        let old = mini();
        let mut new = mini();
        fields_mut(&mut new, "MiniMessageMT").push(Element {
            name: "Extra".to_owned(),
            type_name: "string".to_owned(),
            min_occurs: 0,
            max_occurs: MaxOccurs::One,
            docs: String::new(),
        });
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_ADDED")
            .expect("field added");
        assert_eq!(change.class, Class::Compatible);
    }

    #[test]
    fn detects_required_field_added() {
        let old = mini();
        let mut new = mini();
        fields_mut(&mut new, "MiniMessageMT").push(Element {
            name: "Extra".to_owned(),
            type_name: "string".to_owned(),
            min_occurs: 1,
            max_occurs: MaxOccurs::One,
            docs: String::new(),
        });
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_ADDED")
            .expect("field added");
        assert_eq!(change.class, Class::Breaking);
    }

    #[test]
    fn detects_field_removed() {
        let old = mini();
        let mut new = mini();
        fields_mut(&mut new, "MiniMessageMT").retain(|f| f.name != "Callsign");
        let report = diff(&old, &new);
        assert!(kinds(&report).contains(&"FIELD_REMOVED"));
        assert!(report.has_breaking());
    }

    #[test]
    fn detects_type_change() {
        let old = mini();
        let mut new = mini();
        let fields = fields_mut(&mut new, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Callsign").unwrap();
        field.type_name = "string".to_owned();
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_TYPE_CHANGED")
            .expect("type changed");
        assert_eq!(change.class, Class::Breaking);
    }

    #[test]
    fn detects_optionality_change() {
        let old = mini();
        let mut new = mini();
        let fields = fields_mut(&mut new, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Callsign").unwrap();
        field.min_occurs = 1;
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_OPTIONALITY_CHANGED")
            .expect("optionality changed");
        assert_eq!(change.class, Class::Breaking);
    }

    #[test]
    fn detects_cardinality_change() {
        let old = mini();
        let mut new = mini();
        let fields = fields_mut(&mut new, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Note").unwrap();
        field.max_occurs = MaxOccurs::One;
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_CARDINALITY_CHANGED")
            .expect("cardinality changed");
        assert_eq!(change.class, Class::Breaking);
    }

    #[test]
    fn detects_enum_added_and_removed() {
        let old = mini();
        let mut new = mini();
        {
            let Some(TypeInfo::Simple(SimpleType { enum_values, .. })) =
                new.types.get_mut("ModeEnum")
            else {
                panic!("ModeEnum");
            };
            enum_values.push("NEW".to_owned());
        }
        let report = diff(&old, &new);
        assert!(kinds(&report).contains(&"ENUM_VALUE_ADDED"));
        assert_eq!(
            report
                .changes
                .iter()
                .find(|c| c.kind == "ENUM_VALUE_ADDED")
                .unwrap()
                .class,
            Class::Risky
        );

        {
            let Some(TypeInfo::Simple(SimpleType { enum_values, .. })) =
                new.types.get_mut("ModeEnum")
            else {
                panic!("ModeEnum");
            };
            enum_values.pop();
            enum_values.remove(0);
        }
        let report = diff(&old, &new);
        assert!(kinds(&report).contains(&"ENUM_VALUE_REMOVED"));
        assert!(report.has_breaking());
    }

    #[test]
    fn markdown_contains_summary_and_rows() {
        let old = mini();
        let mut new = mini();
        new.messages
            .insert("Other".to_owned(), old.messages["MiniMessage"].clone());
        let text = diff(&old, &new).to_markdown();
        assert!(text.starts_with("# UCI 002.6.0 to 002.6.0"));
        assert!(text.contains("| compatible | MESSAGE_ADDED |"));
    }

    #[test]
    fn class_labels_and_counts() {
        assert_eq!(Class::Compatible.label(), "compatible");
        assert_eq!(Class::Risky.label(), "risky");
        assert_eq!(Class::Breaking.label(), "breaking");
        let report = DiffReport {
            from: "a".to_owned(),
            to: "b".to_owned(),
            changes: vec![
                Change {
                    kind: "A".to_owned(),
                    class: Class::Compatible,
                    message: None,
                    path: "a".to_owned(),
                    detail: String::new(),
                },
                Change {
                    kind: "B".to_owned(),
                    class: Class::Risky,
                    message: None,
                    path: "b".to_owned(),
                    detail: String::new(),
                },
                Change {
                    kind: "C".to_owned(),
                    class: Class::Breaking,
                    message: None,
                    path: "c".to_owned(),
                    detail: String::new(),
                },
            ],
        };
        assert_eq!(report.counts(), (1, 1, 1));
    }

    #[test]
    fn detects_root_type_change() {
        let old = mini();
        let mut new = mini();
        new.messages.get_mut("MiniMessage").unwrap().root_type = "ChoiceType".to_owned();
        let report = diff(&old, &new);
        assert!(kinds(&report).contains(&"MESSAGE_ROOT_CHANGED"));
        assert!(report.has_breaking());
    }

    #[test]
    fn optionality_widening_is_compatible() {
        let mut old = mini();
        let mut new = mini();
        let fields = fields_mut(&mut old, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Callsign").unwrap();
        field.min_occurs = 1;
        let fields = fields_mut(&mut new, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Callsign").unwrap();
        field.min_occurs = 0;
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_OPTIONALITY_CHANGED")
            .expect("optionality changed");
        assert_eq!(change.class, Class::Compatible);
    }

    #[test]
    fn cardinality_widening_is_compatible() {
        let mut old = mini();
        let mut new = mini();
        let fields = fields_mut(&mut old, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Note").unwrap();
        field.max_occurs = MaxOccurs::Bounded { n: 2 };
        let fields = fields_mut(&mut new, "MiniMessageMT");
        let field = fields.iter_mut().find(|f| f.name == "Note").unwrap();
        field.max_occurs = MaxOccurs::Unbounded;
        let report = diff(&old, &new);
        let change = report
            .changes
            .iter()
            .find(|c| c.kind == "FIELD_CARDINALITY_CHANGED")
            .expect("cardinality changed");
        assert_eq!(change.class, Class::Compatible);
    }

    #[test]
    fn cardinality_matrix() {
        use MaxOccurs::{Bounded, One, Unbounded};
        assert!(!cardinality_narrowed(One, One));
        assert!(!cardinality_narrowed(One, Unbounded));
        assert!(cardinality_narrowed(Unbounded, One));
        assert!(cardinality_narrowed(Unbounded, Bounded { n: 2 }));
        assert!(!cardinality_narrowed(Bounded { n: 2 }, Unbounded));
        assert!(cardinality_narrowed(Bounded { n: 3 }, Bounded { n: 2 }));
        assert!(!cardinality_narrowed(Bounded { n: 2 }, Bounded { n: 3 }));
        assert!(!cardinality_narrowed(Unbounded, Unbounded));
    }

    #[test]
    fn enum_diff_skips_incomparable_types() {
        let mut old = mini();
        let mut new = mini();
        old.types.insert(
            "NoEnum".to_owned(),
            TypeInfo::Simple(SimpleType {
                name: "NoEnum".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: Vec::new(),
            }),
        );
        old.types.insert(
            "Gone".to_owned(),
            TypeInfo::Simple(SimpleType {
                name: "Gone".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: vec!["A".to_owned()],
                facets: Vec::new(),
            }),
        );
        new.types.remove("ModeEnum");
        let report = diff(&old, &new);
        assert!(report.changes.iter().all(|c| c.kind != "ENUM_VALUE_ADDED"));
    }
}
