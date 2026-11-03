//! Generate a static documentation site from a parsed UCI schema.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use heck::ToKebabCase;
use serde_json::{Value, json};
use uci_schema::{MaxOccurs, Schema, TypeInfo};

/// Build options.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options<'a> {
    /// Link a diff page that already exists in the output directory.
    pub include_diff: bool,
    /// Change data used for per-message badges.
    pub changes: Option<&'a uci_diff::DiffReport>,
    /// Version label, for example `2.6`.
    pub version: Option<&'a str>,
    /// Show a link to the other schema version.
    pub switch: bool,
}

/// Build the site into `out`.
pub fn build(schema: &Schema, out: &Path, options: Options<'_>) -> anyhow::Result<BuildReport> {
    let mut report = BuildReport::default();
    let classes = message_classes(options.changes);
    let messages: Vec<String> = schema.messages.keys().cloned().collect();
    let types: Vec<String> = schema
        .types
        .iter()
        .filter(|(_, t)| matches!(t, TypeInfo::Complex(_)))
        .map(|(name, _)| name.clone())
        .collect();

    let message_slugs = slugs(&messages);
    let type_slugs = slugs(&types);
    let search = search_index(schema, &message_slugs, &type_slugs);

    write(out.join("index.html"), &index_page(schema, options))?;
    write(
        out.join("messages/index.html"),
        &messages_page(schema, &message_slugs, options, &classes),
    )?;
    write(
        out.join("types/index.html"),
        &types_page(schema, &type_slugs, options),
    )?;
    write(
        out.join("search-index.json"),
        &serde_json::to_string_pretty(&search)?,
    )?;
    write(out.join("assets/style.css"), STYLE)?;
    write(out.join("assets/search.js"), SEARCH_JS)?;

    for name in &messages {
        let slug = &message_slugs[name];
        let (html, example) = message_page(
            schema,
            name,
            slug,
            &type_slugs,
            options,
            classes.get(name).copied(),
        );
        if example {
            report.examples += 1;
        } else {
            report.missing_examples += 1;
        }
        write(out.join(format!("messages/{slug}.html")), &html)?;
        report.messages += 1;
    }
    for name in &types {
        let slug = &type_slugs[name];
        write(
            out.join(format!("types/{slug}.html")),
            &type_page(schema, name, slug, &type_slugs, options),
        )?;
        report.types += 1;
    }
    Ok(report)
}

/// Build statistics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildReport {
    /// Message pages written.
    pub messages: usize,
    /// Type pages written.
    pub types: usize,
    /// Messages with a schema-valid generated example.
    pub examples: usize,
    /// Messages without a generated example.
    pub missing_examples: usize,
}

fn message_classes(changes: Option<&uci_diff::DiffReport>) -> BTreeMap<String, uci_diff::Class> {
    let mut out: BTreeMap<String, uci_diff::Class> = BTreeMap::new();
    let Some(report) = changes else {
        return out;
    };
    for change in &report.changes {
        let Some(message) = &change.message else {
            continue;
        };
        out.entry(message.clone())
            .and_modify(|current| {
                if change.class > *current {
                    *current = change.class;
                }
            })
            .or_insert(change.class);
    }
    out
}

fn class_badge(class: Option<uci_diff::Class>) -> String {
    let Some(class) = class else {
        return String::new();
    };
    let label = match class {
        uci_diff::Class::Breaking => "breaking change",
        uci_diff::Class::Risky => "risky change",
        uci_diff::Class::Compatible => "compatible change",
    };
    format!("<span class=\"badge {}\">{label}</span>", class.label())
}

fn slugs(names: &[String]) -> BTreeMap<String, String> {
    let mut used = BTreeSet::new();
    let mut out = BTreeMap::new();
    for name in names {
        let mut slug = name.to_kebab_case();
        if slug.is_empty() {
            slug = "unnamed".to_owned();
        }
        let base = slug.clone();
        let mut suffix = 2u32;
        while !used.insert(slug.clone()) {
            slug = format!("{base}-{suffix}");
            suffix += 1;
        }
        out.insert(name.clone(), slug);
    }
    out
}

fn search_index(
    schema: &Schema,
    messages: &BTreeMap<String, String>,
    types: &BTreeMap<String, String>,
) -> Value {
    let mut entries = Vec::new();
    for (name, slug) in messages {
        let info = &schema.messages[name];
        entries.push(json!({
            "kind": "message",
            "name": name,
            "url": format!("messages/{slug}.html"),
            "summary": info.summary,
            "primitive": info.primitive,
        }));
    }
    for name in types.keys() {
        if let Some(TypeInfo::Complex(c)) = schema.types.get(name) {
            entries.push(json!({
                "kind": "type",
                "name": name,
                "url": format!("types/{}.html", types[name]),
                "summary": c.docs.lines().next().unwrap_or_default(),
                "primitive": null,
            }));
        }
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["name"].as_str().unwrap_or_default())
    });
    Value::Array(entries)
}

fn index_page(schema: &Schema, options: Options<'_>) -> String {
    let body = format!(
        "<h1>UCI Forge</h1>\n<p class=\"lede\">Typed bindings, validation, and a mission-thread harness for the public UCI 2.6 schema.</p>\n\
         <dl class=\"stats\">\n\
         <div><dt>Messages</dt><dd>{}</dd></div>\n\
         <div><dt>Complex types</dt><dd>{}</dd></div>\n\
         <div><dt>Simple types</dt><dd>{}</dd></div>\n\
         <div><dt>Enum values</dt><dd>{}</dd></div>\n\
         </dl>\n\
         <p>Start with the <a href=\"messages/index.html\">message index</a> or the <a href=\"types/index.html\">type index</a>.</p>\n\
         <p class=\"note\">Generated from the public UCI {} schema. Not affiliated with or endorsed by the U.S. Air Force.</p>",
        schema.message_count(),
        schema.complex_count(),
        schema.simple_count(),
        schema.enum_value_count(),
        schema.version
    );
    shell("UCI Forge", "", &body, options)
}

fn messages_page(
    schema: &Schema,
    slugs: &BTreeMap<String, String>,
    options: Options<'_>,
    classes: &BTreeMap<String, uci_diff::Class>,
) -> String {
    let mut rows = String::new();
    for (name, slug) in slugs {
        let info = &schema.messages[name];
        rows.push_str(&format!(
            "<tr data-name=\"{}\"><td><a href=\"{slug}.html\">{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&name.to_lowercase()),
            escape(name),
            escape(info.primitive.as_deref().unwrap_or("")),
            escape(&info.summary),
            class_badge(classes.get(name).copied())
        ));
    }
    let body = format!(
        "<h1>Messages</h1>\n\
         <p class=\"lede\">{} message roots in UCI {}.</p>\n\
         <label class=\"search\">Filter messages <input id=\"filter\" type=\"search\" autocomplete=\"off\" spellcheck=\"false\"></label>\n\
         <div class=\"table-wrap\"><table class=\"index\">\n\
         <thead><tr><th scope=\"col\">Message</th><th scope=\"col\">Primitive</th><th scope=\"col\">Summary</th><th scope=\"col\">Change</th></tr></thead>\n\
         <tbody id=\"rows\">{rows}</tbody></table></div>\n\
         <p id=\"empty\" class=\"note\" hidden>No message matches that filter.</p>",
        schema.message_count(),
        schema.version
    );
    shell("Messages", "../", &body, options)
}

fn types_page(schema: &Schema, slugs: &BTreeMap<String, String>, options: Options<'_>) -> String {
    let mut rows = String::new();
    for (name, slug) in slugs {
        let Some(TypeInfo::Complex(c)) = schema.types.get(name) else {
            continue;
        };
        rows.push_str(&format!(
            "<tr data-name=\"{}\"><td><a href=\"{slug}.html\">{}</a></td><td>{}</td></tr>\n",
            escape(&name.to_lowercase()),
            escape(name),
            escape(c.docs.lines().next().unwrap_or_default())
        ));
    }
    let body = format!(
        "<h1>Types</h1>\n\
         <p class=\"lede\">{} complex types in UCI {}.</p>\n\
         <label class=\"search\">Filter types <input id=\"filter\" type=\"search\" autocomplete=\"off\" spellcheck=\"false\"></label>\n\
         <div class=\"table-wrap\"><table class=\"index\">\n\
         <thead><tr><th scope=\"col\">Type</th><th scope=\"col\">Summary</th></tr></thead>\n\
         <tbody id=\"rows\">{rows}</tbody></table></div>\n\
         <p id=\"empty\" class=\"note\" hidden>No type matches that filter.</p>",
        slugs.len(),
        schema.version
    );
    shell("Types", "../", &body, options)
}

fn message_page(
    schema: &Schema,
    name: &str,
    _slug: &str,
    type_slugs: &BTreeMap<String, String>,
    options: Options<'_>,
    class: Option<uci_diff::Class>,
) -> (String, bool) {
    let info = &schema.messages[name];
    let fields = schema.fields(&info.root_type);
    let required = fields.iter().filter(|f| f.min_occurs > 0).count();
    let sample = sample::message(schema, name);
    let example = sample
        .as_ref()
        .and_then(|v| serde_json::to_string_pretty(v).ok());
    let valid = sample
        .as_ref()
        .is_some_and(|v| uci_validate::validate(schema, v).is_ok());
    let example_html = if valid {
        format!(
            "<h2>Example</h2>\n<p class=\"note\">Generated from required fields. Validate before use.</p>\n<pre><code>{}</code></pre>",
            escape(&example.unwrap_or_default())
        )
    } else {
        "<h2>Example</h2>\n<p class=\"note\">No valid example was generated for this message.</p>"
            .to_owned()
    };
    let docs_html = if info.docs.trim().is_empty() || info.docs.trim() == info.summary.trim() {
        String::new()
    } else {
        format!("<pre class=\"docs\">{}</pre>\n", escape(&info.docs))
    };
    let body = format!(
        "<p class=\"crumb\"><a href=\"../messages/index.html\">Messages</a></p>\n\
         <header class=\"msg-head\">\n\
         <h1>{}</h1>\n\
         <dl>\n\
         <div><dt>Primitive</dt><dd>{}</dd></div>\n\
         <div><dt>Version</dt><dd>{}</dd></div>\n\
         <div><dt>Required</dt><dd>{} fields</dd></div>\n\
         </dl>\n\
         {badge}\n\
         </header>\n\
         <p class=\"lede\">{}</p>\n\
         {docs_html}\
         <h2>Fields</h2>\n\
         {}\n\
         {example_html}",
        escape(name),
        escape(info.primitive.as_deref().unwrap_or("none")),
        escape(&schema.version),
        required,
        escape(&info.summary),
        fields_table(&fields, type_slugs),
        badge = class_badge(class),
        docs_html = docs_html
    );
    (shell(name, "../", &body, options), valid)
}

fn type_page(
    schema: &Schema,
    name: &str,
    _slug: &str,
    type_slugs: &BTreeMap<String, String>,
    options: Options<'_>,
) -> String {
    let Some(TypeInfo::Complex(c)) = schema.types.get(name) else {
        return shell("Unknown type", "../", "<h1>Unknown type</h1>", options);
    };
    let fields = schema.fields(name);
    let base = c
        .base
        .as_ref()
        .map(|b| format!("<p>Extends {}</p>", type_link(b, type_slugs)))
        .unwrap_or_default();
    let subtypes = if c.subtypes.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = c
            .subtypes
            .iter()
            .map(|s| format!("<li>{}</li>", type_link(s, type_slugs)))
            .collect();
        format!("<h2>Subtypes</h2><ul>{}</ul>", items.join(""))
    };
    let summary = c.docs.lines().next().unwrap_or_default();
    let docs_html = if c.docs.trim().is_empty() || c.docs.trim() == summary.trim() {
        String::new()
    } else {
        format!("<pre class=\"docs\">{}</pre>\n", escape(&c.docs))
    };
    let body = format!(
        "<p class=\"crumb\"><a href=\"../types/index.html\">Types</a></p>\n\
         <header class=\"msg-head\"><h1>{}</h1>\n\
         <dl><div><dt>Kind</dt><dd>complex{}</dd></div><div><dt>Version</dt><dd>{}</dd></div><div><dt>Fields</dt><dd>{}</dd></div></dl>\n\
         </header>\n\
         {base}\n\
         <p class=\"lede\">{}</p>\n\
         {docs_html}\
         <h2>Fields</h2>\n\
         {}\n\
         {subtypes}",
        escape(name),
        if c.abstract_ { ", abstract" } else { "" },
        escape(&schema.version),
        fields.len(),
        escape(summary),
        fields_table(&fields, type_slugs),
        docs_html = docs_html
    );
    shell(name, "../", &body, options)
}

fn fields_table(fields: &[uci_schema::Element], type_slugs: &BTreeMap<String, String>) -> String {
    if fields.is_empty() {
        return "<p class=\"note\">No fields.</p>".to_owned();
    }
    let mut rows = String::new();
    for field in fields {
        let card = match field.max_occurs {
            MaxOccurs::One => {
                if field.min_occurs == 0 {
                    "0..1".to_owned()
                } else {
                    "1".to_owned()
                }
            }
            MaxOccurs::Bounded { n } => format!("{}..{n}", field.min_occurs),
            MaxOccurs::Unbounded => format!("{}..*", field.min_occurs),
        };
        let required = if field.min_occurs > 0 {
            "<span class=\"req\">required</span>"
        } else {
            "<span class=\"opt\">optional</span>"
        };
        rows.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&field.name),
            type_link(&field.type_name, type_slugs),
            card,
            required
        ));
    }
    format!(
        "<div class=\"table-wrap\"><table>\n\
         <thead><tr><th scope=\"col\">Field</th><th scope=\"col\">Type</th><th scope=\"col\">Cardinality</th><th scope=\"col\">Presence</th></tr></thead>\n\
         <tbody>{rows}</tbody></table></div>"
    )
}

fn type_link(name: &str, slugs: &BTreeMap<String, String>) -> String {
    if let Some(slug) = slugs.get(name) {
        format!(
            "<a href=\"../types/{slug}.html\"><code>{}</code></a>",
            escape(name)
        )
    } else {
        format!("<code>{}</code>", escape(name))
    }
}

fn shell(title: &str, base: &str, body: &str, options: Options<'_>) -> String {
    let diff = if options.include_diff {
        format!("<a href=\"{base}diff/2.5-to-2.6.html\">Diff</a>")
    } else {
        String::new()
    };
    let version = options.version.unwrap_or("2.6");
    let version_tag = format!("<span class=\"version\">v{version}</span>");
    let switch = if options.switch {
        let (href, label) = if version == "2.5" {
            (format!("{base}../index.html"), "v2.6")
        } else {
            (format!("{base}v2.5/index.html"), "v2.5")
        };
        format!("<a class=\"switch\" href=\"{href}\">{label}</a>")
    } else {
        String::new()
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title} · UCI Forge v{version}</title>\n<link rel=\"stylesheet\" href=\"{base}assets/style.css\">\n</head>\n<body>\n<a class=\"skip\" href=\"#main\">Skip to content</a>\n<header class=\"site\">\n<a class=\"brand\" href=\"{base}index.html\">UCI Forge</a>{version_tag}\n<nav aria-label=\"Primary\"><a href=\"{base}messages/index.html\">Messages</a><a href=\"{base}types/index.html\">Types</a>{diff}</nav>\n{switch}\n<button id=\"theme\" type=\"button\" aria-pressed=\"false\">Dark</button>\n</header>\n<main id=\"main\">\n{body}\n</main>\n<footer><p>Generated from the public UCI {version} schema. Not operational. Not affiliated with or endorsed by the U.S. Air Force.</p></footer>\n<script src=\"{base}assets/search.js\" defer></script>\n</body>\n</html>\n"
    )
}

/// Write a diff page for an existing report.
pub fn write_diff_page(report: &uci_diff::DiffReport, out: &Path) -> anyhow::Result<()> {
    let (compatible, risky, breaking) = report.counts();
    let mut rows = String::new();
    for change in &report.changes {
        rows.push_str(&format!(
            "<tr><td><span class=\"badge {}\">{}</span></td><td><code>{}</code></td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>\n",
            change.class.label(),
            change.class.label(),
            escape(&change.kind),
            escape(change.message.as_deref().unwrap_or("")),
            escape(&change.path),
            escape(&change.detail)
        ));
    }
    let body = format!(
        "<h1>Diff</h1>\n<p class=\"lede\">UCI {} to {}: {} breaking, {} risky, {} compatible.</p>\n         <div class=\"table-wrap\"><table>\n         <thead><tr><th scope=\"col\">Class</th><th scope=\"col\">Kind</th><th scope=\"col\">Message</th><th scope=\"col\">Path</th><th scope=\"col\">Detail</th></tr></thead>\n         <tbody>{rows}</tbody></table></div>\n         <p class=\"note\">Generated by <code>uci-diff</code>. Breaking means at least one side of the wire contract must change.</p>",
        escape(&report.from),
        escape(&report.to),
        breaking,
        risky,
        compatible
    );
    write(
        out.join("diff/2.5-to-2.6.html"),
        &shell(
            "Diff",
            "../",
            &body,
            Options {
                include_diff: true,
                version: Some("2.6"),
                ..Default::default()
            },
        ),
    )?;
    write(
        out.join("diff/2.5-to-2.6.json"),
        &serde_json::to_string_pretty(report)?,
    )?;
    Ok(())
}

fn write(path: std::path::PathBuf, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = r#":root{color-scheme:light;--bg:#f7f4ee;--ink:#1c1b18;--muted:#5a564d;--rule:#d8d2c4;--accent:#9a3b00;--code:#efe9dc}
html[data-theme=dark]{color-scheme:dark;--bg:#14130f;--ink:#ece7db;--muted:#b3ac9c;--rule:#35322a;--accent:#ffb27a;--code:#1e1c16}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:16px/1.55 Charter,"Iowan Old Style",Georgia,serif}
a{color:var(--accent);text-underline-offset:.18em}
a:focus-visible,button:focus-visible,input:focus-visible{outline:3px solid var(--accent);outline-offset:2px}
.skip{position:absolute;left:-999px;top:0;background:var(--bg);padding:.6rem 1rem}
.skip:focus{left:1rem;z-index:10}
.site{display:flex;align-items:center;gap:1.2rem;flex-wrap:wrap;border-bottom:1px solid var(--rule);padding:.8rem 1.2rem;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.9rem}
.brand{font-weight:700;text-decoration:none;color:var(--ink)}
.version{font-size:.78rem;color:var(--muted)}
.switch{display:inline-flex;align-items:center;min-height:44px;padding:0 .4rem;border:1px solid var(--rule);color:var(--ink);text-decoration:none}
nav{display:flex;gap:.4rem}
nav a{text-decoration:none;display:inline-flex;align-items:center;min-height:44px;padding:0 .4rem}
#theme{margin-left:auto;min-height:44px;min-width:44px;background:none;border:1px solid var(--rule);color:var(--ink);font:inherit;cursor:pointer;padding:.4rem .7rem}
main{max-width:72rem;margin:0 auto;padding:2rem 1.2rem 4rem}
h1{font-size:2rem;line-height:1.15;margin:.2rem 0 .6rem}
h2{font-size:1.15rem;margin:2rem 0 .6rem;border-bottom:1px solid var(--rule);padding-bottom:.3rem}
.lede{font-size:1.1rem;max-width:60ch}
.note,.crumb{color:var(--muted);font-size:.9rem}
.crumb{margin:0}
.msg-head{border-left:3px solid var(--accent);padding-left:1rem;margin:0 0 1.4rem}
.msg-head dl{display:flex;gap:1.4rem;flex-wrap:wrap;margin:.4rem 0 0;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.82rem}
.msg-head dt{color:var(--muted)}
.msg-head dd{margin:0}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(9rem,1fr));gap:1rem;padding:0;margin:1.6rem 0}
.stats div{border-top:1px solid var(--rule);padding-top:.5rem}
.stats dt{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.78rem;color:var(--muted)}
.stats dd{margin:0;font-size:1.6rem}
.table-wrap{overflow-x:auto;border:1px solid var(--rule)}
table{width:100%;border-collapse:collapse;font-size:.92rem}
th,td{text-align:left;padding:.55rem .7rem;border-bottom:1px solid var(--rule);vertical-align:top}
th{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.78rem;font-weight:600;color:var(--muted)}
tbody tr:last-child td{border-bottom:0}
code{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.88em;background:var(--code);padding:.08em .28em}
pre{background:var(--code);border:1px solid var(--rule);padding:1rem;overflow-x:auto;font-size:.86rem;line-height:1.45}
pre code{background:none;padding:0}
.docs{white-space:pre-wrap;font-family:inherit;font-size:.95rem}
.req{color:var(--accent);font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.78rem}
.opt{color:var(--muted);font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.78rem}
.search{display:block;margin:1rem 0;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.85rem}
.search input{display:block;width:100%;max-width:34rem;margin-top:.35rem;min-height:44px;padding:.5rem .6rem;border:1px solid var(--rule);background:var(--bg);color:var(--ink);font:inherit}
footer{border-top:1px solid var(--rule);padding:1.2rem;color:var(--muted);font-size:.82rem;max-width:72rem;margin:0 auto}
@media (max-width:640px){.site{gap:.6rem}.msg-head dl{gap:.8rem}h1{font-size:1.6rem}main{padding-top:1.2rem}}
.badge{display:inline-block;padding:.1rem .45rem;border:1px solid currentColor;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:.76rem}
.badge.breaking{color:#a11b1b}
.badge.risky{color:#8a5a00}
.badge.compatible{color:#2f5d3a}
html[data-theme=dark] .badge.breaking{color:#ff9c9c}
html[data-theme=dark] .badge.risky{color:#ffd28a}
html[data-theme=dark] .badge.compatible{color:#9fdcaf}
@media (prefers-reduced-motion:reduce){*{transition:none!important;animation:none!important}}
"#;

const SEARCH_JS: &str = r#"const input=document.getElementById('filter');
const rows=[...document.querySelectorAll('#rows tr')];
const empty=document.getElementById('empty');
if(input){input.addEventListener('input',()=>{const q=input.value.trim().toLowerCase();let shown=0;for(const row of rows){const hit=row.dataset.name.includes(q);row.hidden=!hit;if(hit)shown++;}if(empty)empty.hidden=shown!==0;});}
const theme=document.getElementById('theme');
const root=document.documentElement;
if(theme){const saved=localStorage.getItem('uci-theme');if(saved==='dark'||(!saved&&matchMedia('(prefers-color-scheme:dark)').matches)){root.dataset.theme='dark';theme.setAttribute('aria-pressed','true');theme.textContent='Light';}theme.addEventListener('click',()=>{const dark=root.dataset.theme!=='dark';root.dataset.theme=dark?'dark':'';theme.setAttribute('aria-pressed',String(dark));theme.textContent=dark?'Light':'Dark';localStorage.setItem('uci-theme',dark?'dark':'light');});}
"#;

mod sample;

/// Build one schema-valid example message, when possible.
#[must_use]
pub fn example_message(schema: &Schema, name: &str) -> Option<Value> {
    sample::message(schema, name)
}

/// Diagnostics for the example generator.
pub fn debug_message(
    schema: &Schema,
    name: &str,
) -> Result<Value, Vec<uci_validate::ValidationError>> {
    let value = sample::message_unchecked(schema, name).ok_or_else(|| {
        vec![uci_validate::ValidationError {
            path: "$".to_owned(),
            code: "SAMPLE".to_owned(),
            expected: None,
            found: None,
        }]
    })?;
    uci_validate::validate(schema, &value)?;
    Ok(value)
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
    fn builds_mini_site() {
        let dir = std::env::temp_dir().join(format!("uci-docs-mini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = build(&mini(), &dir, Options::default()).expect("build");
        assert_eq!(report.messages, 1);
        assert!(dir.join("messages/mini-message.html").exists());
        assert!(dir.join("search-index.json").exists());
        let html = std::fs::read_to_string(dir.join("messages/mini-message.html")).expect("read");
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("<main id=\"main\">"));
        assert!(html.contains("<h1>MiniMessage</h1>"));
        assert!(!html.contains("—"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slugs_are_unique() {
        let names = vec!["A_B".to_owned(), "A-B".to_owned()];
        let map = slugs(&names);
        assert_eq!(map.len(), 2);
        assert_ne!(map["A_B"], map["A-B"]);
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape("<a&b>"), "&lt;a&amp;b&gt;");
    }

    #[test]
    fn slugs_empty_name() {
        let map = slugs(&["".to_owned()]);
        assert_eq!(map[""], "unnamed");
    }

    #[test]
    fn skips_non_complex_types_in_index() {
        let mut map = BTreeMap::new();
        map.insert("ModeEnum".to_owned(), "mode-enum".to_owned());
        let html = types_page(&mini(), &map, Options::default());
        assert!(html.contains("Types"));
    }

    #[test]
    fn unknown_type_page_is_honest() {
        let html = type_page(
            &mini(),
            "Nope",
            "nope",
            &BTreeMap::new(),
            Options::default(),
        );
        assert!(html.contains("Unknown type"));
    }

    #[test]
    fn empty_fields_table() {
        let html = fields_table(&[], &BTreeMap::new());
        assert!(html.contains("No fields."));
    }

    #[test]
    fn cardinality_labels_cover_all_shapes() {
        let fields = vec![
            uci_schema::Element {
                name: "One".to_owned(),
                type_name: "string".to_owned(),
                min_occurs: 0,
                max_occurs: MaxOccurs::One,
                docs: String::new(),
            },
            uci_schema::Element {
                name: "Bounded".to_owned(),
                type_name: "string".to_owned(),
                min_occurs: 0,
                max_occurs: MaxOccurs::Bounded { n: 3 },
                docs: String::new(),
            },
            uci_schema::Element {
                name: "Many".to_owned(),
                type_name: "string".to_owned(),
                min_occurs: 1,
                max_occurs: MaxOccurs::Unbounded,
                docs: String::new(),
            },
        ];
        let html = fields_table(&fields, &BTreeMap::new());
        assert!(html.contains("0..1"));
        assert!(html.contains("0..3"));
        assert!(html.contains("1..*"));
    }

    #[test]
    fn includes_diff_in_navigation() {
        let dir = std::env::temp_dir().join(format!("uci-docs-diffnav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        build(
            &mini(),
            &dir,
            Options {
                include_diff: true,
                ..Default::default()
            },
        )
        .unwrap();
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(html.contains("diff/2.5-to-2.6.html"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_diff_page_for_every_class() {
        let dir = std::env::temp_dir().join(format!("uci-docs-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = uci_diff::DiffReport {
            from: "002.5.0".to_owned(),
            to: "002.6.0".to_owned(),
            changes: vec![
                uci_diff::Change {
                    kind: "MESSAGE_REMOVED".to_owned(),
                    class: uci_diff::Class::Breaking,
                    message: Some("Gone".to_owned()),
                    path: "Gone".to_owned(),
                    detail: "removed".to_owned(),
                },
                uci_diff::Change {
                    kind: "ENUM_VALUE_ADDED".to_owned(),
                    class: uci_diff::Class::Risky,
                    message: None,
                    path: "type.Mode".to_owned(),
                    detail: "added".to_owned(),
                },
                uci_diff::Change {
                    kind: "MESSAGE_ADDED".to_owned(),
                    class: uci_diff::Class::Compatible,
                    message: Some("New".to_owned()),
                    path: "New".to_owned(),
                    detail: "added".to_owned(),
                },
            ],
        };
        write_diff_page(&report, &dir).unwrap();
        let html = std::fs::read_to_string(dir.join("diff/2.5-to-2.6.html")).unwrap();
        assert!(html.contains("badge breaking"));
        assert!(html.contains("badge risky"));
        assert!(html.contains("badge compatible"));
        assert!(dir.join("diff/2.5-to-2.6.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_missing_examples() {
        let mut schema = mini();
        schema.types.insert(
            "Impossible".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Impossible".to_owned(),
                base: "string".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![
                    uci_schema::Facet {
                        kind: "length".to_owned(),
                        value: "3".to_owned(),
                    },
                    uci_schema::Facet {
                        kind: "pattern".to_owned(),
                        value: "^[A-Z]{3}$".to_owned(),
                    },
                ],
            }),
        );
        let Some(TypeInfo::Complex(complex)) = schema.types.get_mut("MiniMessageMT") else {
            panic!("MiniMessageMT");
        };
        if let uci_schema::Content::Sequence { elements } = &mut complex.content {
            let choice = elements.iter_mut().find(|e| e.name == "Choice").unwrap();
            choice.type_name = "Impossible".to_owned();
        }
        let dir = std::env::temp_dir().join(format!("uci-docs-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = build(&schema, &dir, Options::default()).unwrap();
        assert_eq!(report.missing_examples, 1);
        let html = std::fs::read_to_string(dir.join("messages/mini-message.html")).unwrap();
        assert!(html.contains("No valid example"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn example_helpers_report_success_and_failure() {
        let schema = mini();
        assert!(example_message(&schema, "MiniMessage").is_some());
        assert!(example_message(&schema, "Nope").is_none());
        assert!(debug_message(&schema, "MiniMessage").is_ok());
        assert!(debug_message(&schema, "Nope").is_err());
    }

    #[test]
    fn skips_duplicate_docs() {
        let mut schema = mini();
        let message = schema.messages.get_mut("MiniMessage").unwrap();
        message.docs = "Same text".to_owned();
        message.summary = "Same text".to_owned();
        let dir = std::env::temp_dir().join(format!("uci-docs-dupe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        build(&schema, &dir, Options::default()).unwrap();
        let html = std::fs::read_to_string(dir.join("messages/mini-message.html")).unwrap();
        assert!(!html.contains("<pre class=\"docs\">"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_reports_io_errors() {
        let dir = std::env::temp_dir().join(format!("uci-docs-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "x").unwrap();
        assert!(write(blocked.join("x"), "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn numeric_facets_shape_examples() {
        let mut schema = Schema::default();
        schema.types.insert(
            "Positive".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Positive".to_owned(),
                base: "unsignedInt".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![uci_schema::Facet {
                    kind: "minInclusive".to_owned(),
                    value: "1".to_owned(),
                }],
            }),
        );
        schema.types.insert(
            "Ratio".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Ratio".to_owned(),
                base: "double".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![uci_schema::Facet {
                    kind: "minInclusive".to_owned(),
                    value: "0.5".to_owned(),
                }],
            }),
        );
        schema.types.insert(
            "Negative".to_owned(),
            TypeInfo::Simple(uci_schema::SimpleType {
                name: "Negative".to_owned(),
                base: "int".to_owned(),
                docs: String::new(),
                enum_values: Vec::new(),
                facets: vec![uci_schema::Facet {
                    kind: "maxInclusive".to_owned(),
                    value: "-1".to_owned(),
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
                    elements: vec![
                        uci_schema::Element {
                            name: "Count".to_owned(),
                            type_name: "Positive".to_owned(),
                            min_occurs: 1,
                            max_occurs: MaxOccurs::One,
                            docs: String::new(),
                        },
                        uci_schema::Element {
                            name: "Ratio".to_owned(),
                            type_name: "Ratio".to_owned(),
                            min_occurs: 1,
                            max_occurs: MaxOccurs::One,
                            docs: String::new(),
                        },
                        uci_schema::Element {
                            name: "Floor".to_owned(),
                            type_name: "Negative".to_owned(),
                            min_occurs: 1,
                            max_occurs: MaxOccurs::One,
                            docs: String::new(),
                        },
                    ],
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
        let value = example_message(&schema, "Msg").expect("example");
        assert_eq!(value["Msg"]["Count"], serde_json::json!(1));
        assert_eq!(value["Msg"]["Ratio"], serde_json::json!(0.5));
        assert_eq!(value["Msg"]["Floor"], serde_json::json!(-1));
    }

    #[test]
    fn version_switcher_and_change_badges() {
        let report = uci_diff::DiffReport {
            from: "002.5.0".to_owned(),
            to: "002.6.0".to_owned(),
            changes: vec![
                uci_diff::Change {
                    kind: "FIELD_ADDED".to_owned(),
                    class: uci_diff::Class::Compatible,
                    message: Some("MiniMessage".to_owned()),
                    path: "MiniMessage.Extra".to_owned(),
                    detail: "added".to_owned(),
                },
                uci_diff::Change {
                    kind: "FIELD_REMOVED".to_owned(),
                    class: uci_diff::Class::Breaking,
                    message: Some("MiniMessage".to_owned()),
                    path: "MiniMessage.Callsign".to_owned(),
                    detail: "removed".to_owned(),
                },
                uci_diff::Change {
                    kind: "ENUM_VALUE_ADDED".to_owned(),
                    class: uci_diff::Class::Risky,
                    message: None,
                    path: "type.ModeEnum".to_owned(),
                    detail: "added".to_owned(),
                },
            ],
        };
        let dir = std::env::temp_dir().join(format!("uci-docs-badges-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        build(
            &mini(),
            &dir,
            Options {
                include_diff: false,
                changes: Some(&report),
                version: Some("2.6"),
                switch: true,
            },
        )
        .unwrap();
        let page = std::fs::read_to_string(dir.join("messages/mini-message.html")).unwrap();
        assert!(page.contains("breaking change"));
        assert!(page.contains("v2.5/index.html"));
        let index = std::fs::read_to_string(dir.join("messages/index.html")).unwrap();
        assert!(index.contains("breaking change"));
        assert!(index.contains("v2.5/index.html"));

        build(
            &mini(),
            &dir.join("v2.5"),
            Options {
                include_diff: false,
                changes: None,
                version: Some("2.5"),
                switch: true,
            },
        )
        .unwrap();
        let old = std::fs::read_to_string(dir.join("v2.5/index.html")).unwrap();
        assert!(old.contains("../index.html"));
        assert!(old.contains("v2.5"));
        assert_eq!(
            class_badge(Some(uci_diff::Class::Risky)),
            "<span class=\"badge risky\">risky change</span>"
        );
        assert_eq!(
            class_badge(Some(uci_diff::Class::Compatible)),
            "<span class=\"badge compatible\">compatible change</span>"
        );
        assert_eq!(class_badge(None), "");
    }
}
