//! Renderers — Go-style text, Markdown, and HTML.

use crate::extract::linkify_doc_text;
use crate::ir::*;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// Render a [`DocPackage`] to a single Go-style text dump.
pub fn text(doc: &DocPackage) -> String {
    let mut s = String::new();
    s.push_str(&format!("package {}\n", doc.name));
    if !doc.synopsis.is_empty() {
        s.push('\n');
        for line in wrap(&doc.synopsis, 78) {
            s.push_str("    ");
            s.push_str(&line);
            s.push('\n');
        }
    }

    // Group by section header, preserving insertion order via a BTree on
    // a manually-numbered key.
    let groups = group_by_section(&doc.items);
    for (header, items) in groups {
        s.push('\n');
        s.push_str(&header);
        s.push('\n');
        for it in items {
            s.push('\n');
            s.push_str(&first_signature_line(&it.signature.plain));
            s.push('\n');
            if !it.synopsis.is_empty() {
                for line in wrap(&it.synopsis, 74) {
                    s.push_str("        ");
                    s.push_str(&line);
                    s.push('\n');
                }
            }
        }
    }
    s
}

/// Render a single item's full doc, Go-style (used by `sdust doc Item`).
pub fn item_text(doc: &DocPackage, item: &DocItem) -> String {
    let mut s = String::new();
    s.push_str(&format!("package {}\n\n", doc.name));
    s.push_str(&item.signature.plain);
    s.push('\n');
    if !item.body.is_empty() {
        s.push('\n');
        for line in item.body.lines() {
            s.push_str("    ");
            s.push_str(line);
            s.push('\n');
        }
    } else if !item.synopsis.is_empty() {
        s.push('\n');
        for line in wrap(&item.synopsis, 78) {
            s.push_str("    ");
            s.push_str(&line);
            s.push('\n');
        }
    }
    if let Some(since) = &item.since {
        s.push('\n');
        s.push_str(&format!("    Since: {}\n", since));
    }
    if !item.used_by.is_empty() {
        s.push('\n');
        s.push_str("    Used by:\n");
        for u in &item.used_by {
            s.push_str(&format!("        {}\n", u));
        }
    }
    s
}

/// Render the full package as markdown. Returns a map of relative
/// path → file contents that the caller can write to disk.
pub fn markdown(doc: &DocPackage) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("index.md".to_string(), markdown_index(doc));
    for item in &doc.items {
        let path = format!("{}.md", item.anchor);
        out.insert(path, markdown_item(doc, item));
    }
    out
}

fn markdown_index(doc: &DocPackage) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Package `{}`\n\n", doc.name));
    if !doc.body.is_empty() {
        s.push_str(&doc.body);
        s.push_str("\n\n");
    }
    let groups = group_by_section(&doc.items);
    for (header, items) in groups {
        s.push_str(&format!("## {}\n\n", header));
        for it in items {
            s.push_str(&format!(
                "- [`{}`]({}.md) — {}\n",
                it.name,
                it.anchor,
                escape_md_inline(&it.synopsis)
            ));
        }
        s.push('\n');
    }
    s
}

fn markdown_item(doc: &DocPackage, item: &DocItem) -> String {
    let mut s = String::new();
    s.push_str(&format!("# `{}` ({})\n\n", item.name, item.kind.tag()));
    s.push_str("```\n");
    s.push_str(&item.signature.plain);
    s.push_str("\n```\n\n");
    if let Some(since) = &item.since {
        s.push_str(&format!("**Since:** {}\n\n", since));
    }
    if !item.body.is_empty() {
        let item_refs: Vec<&DocItem> = doc.items.iter().collect();
        s.push_str(&linkify_doc_text(&item.body, &item_refs));
        s.push_str("\n\n");
    }
    if !item.examples.is_empty() {
        s.push_str("## Examples\n\n");
        for ex in &item.examples {
            s.push_str(&format!("```{}\n{}\n```\n\n", ex.language, ex.code));
        }
    }
    if !item.used_by.is_empty() {
        s.push_str("## Used by\n\n");
        for u in &item.used_by {
            s.push_str(&format!("- `{}`\n", u));
        }
        s.push('\n');
    }
    s.push_str(&format!("[Back to index](index.md)\n"));
    s
}

/// Render the full package as an HTML site. Returns a map of relative
/// path → file contents.
pub fn html(doc: &DocPackage) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("index.html".to_string(), html_index(doc));
    for item in &doc.items {
        let path = format!("{}.html", item.anchor);
        out.insert(path, html_item(doc, item));
    }
    out.insert("style.css".to_string(), STYLE_CSS.to_string());
    out.insert("search.js".to_string(), SEARCH_JS.to_string());
    out.insert("search-index.json".to_string(), search_index(doc));
    out
}

fn html_index(doc: &DocPackage) -> String {
    let mut body = String::new();
    body.push_str(&format!("<h1>Package <code>{}</code></h1>", esc(&doc.name)));
    if !doc.body.is_empty() {
        body.push_str("<div class=\"pkg-doc\">");
        body.push_str(&render_markdown_to_html(&doc.body));
        body.push_str("</div>");
    }
    body.push_str("<div class=\"search\"><input id=\"q\" type=\"search\" placeholder=\"Search items…\" autofocus /><ul id=\"results\"></ul></div>");
    let groups = group_by_section(&doc.items);
    for (header, items) in groups {
        body.push_str(&format!("<h2>{}</h2><ul class=\"item-list\">", esc(&header)));
        for it in items {
            body.push_str(&format!(
                "<li><a href=\"{}.html\"><code>{}</code></a> <span class=\"syn\">— {}</span></li>",
                it.anchor,
                esc(&it.name),
                esc(&it.synopsis)
            ));
        }
        body.push_str("</ul>");
    }
    page(&doc.name, &doc.name, &body, ".")
}

fn html_item(doc: &DocPackage, item: &DocItem) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<nav class=\"crumbs\"><a href=\"index.html\">{}</a> &rsaquo; <code>{}</code></nav>",
        esc(&doc.name),
        esc(&item.name)
    ));
    body.push_str(&format!(
        "<h1><span class=\"kind\">{}</span> <code>{}</code></h1>",
        esc(item.kind.tag()),
        esc(&item.name)
    ));
    if let Some(since) = &item.since {
        body.push_str(&format!(
            "<p class=\"since\"><em>Since {}</em></p>",
            esc(since)
        ));
    }
    body.push_str("<pre class=\"sig\"><code>");
    body.push_str(&item.signature.html);
    body.push_str("</code></pre>");
    if !item.body.is_empty() {
        let item_refs: Vec<&DocItem> = doc.items.iter().collect();
        let linked = linkify_doc_text(&item.body, &item_refs);
        body.push_str("<div class=\"doc\">");
        body.push_str(&render_markdown_to_html(&linked));
        body.push_str("</div>");
    }
    if !item.examples.is_empty() {
        body.push_str("<h2>Examples</h2>");
        for ex in &item.examples {
            body.push_str("<pre class=\"example\"><code>");
            body.push_str(&esc(&ex.code));
            body.push_str("</code></pre>");
        }
    }
    if !item.used_by.is_empty() {
        body.push_str("<h2>Used by</h2><ul class=\"used-by\">");
        for u in &item.used_by {
            body.push_str(&format!("<li><code>{}</code></li>", esc(u)));
        }
        body.push_str("</ul>");
    }
    page(
        &format!("{} — {}", item.name, doc.name),
        &doc.name,
        &body,
        ".",
    )
}

fn search_index(doc: &DocPackage) -> String {
    // Hand-encoded JSON (no serde_json dep).
    let mut s = String::from("[");
    for (i, it) in doc.items.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"name\":\"{}\",\"kind\":\"{}\",\"url\":\"{}.html\",\"synopsis\":\"{}\"}}",
            json_escape(&it.name),
            json_escape(it.kind.tag()),
            json_escape(&it.anchor),
            json_escape(&it.synopsis)
        ));
    }
    s.push(']');
    s
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn page(title: &str, pkg_name: &str, body: &str, _root: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"/><title>{}</title>\
<link rel=\"stylesheet\" href=\"style.css\"/></head>\
<body><header><a class=\"home\" href=\"index.html\">{}</a></header>\
<main>{}</main>\
<script src=\"search.js\"></script>\
</body></html>",
        esc(title),
        esc(pkg_name),
        body
    )
}

/// Write a rendered tree (markdown or html) to `out_dir`. Creates the
/// directory if it doesn't exist.
pub fn write_tree(out_dir: &Path, files: &BTreeMap<String, String>) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    for (rel, contents) in files {
        let path = out_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn group_by_section(items: &[DocItem]) -> Vec<(String, Vec<&DocItem>)> {
    // Stable section order: FUNCTIONS, TYPES, TRAITS, AGENTS, PROTOCOLS,
    // SUPERVISORS, CONSTANTS.
    let order = [
        "FUNCTIONS",
        "TYPES",
        "TRAITS",
        "AGENTS",
        "PROTOCOLS",
        "SUPERVISORS",
        "CONSTANTS",
    ];
    let mut groups: BTreeMap<&'static str, Vec<&DocItem>> = BTreeMap::new();
    for it in items {
        if it.visibility == DocVisibility::Private {
            continue;
        }
        groups.entry(it.kind.section_header()).or_default().push(it);
    }
    let mut out: Vec<(String, Vec<&DocItem>)> = Vec::new();
    for name in order {
        if let Some(v) = groups.remove(name) {
            out.push((name.to_string(), v));
        }
    }
    // Any unexpected sections last.
    for (k, v) in groups {
        out.push((k.to_string(), v));
    }
    out
}

fn first_signature_line(sig: &str) -> String {
    sig.lines().next().unwrap_or("").to_string()
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
            } else if line.len() + 1 + word.len() > width {
                out.push(std::mem::take(&mut line));
                line.push_str(word);
            } else {
                line.push(' ');
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

fn esc(s: &str) -> String {
    crate::extract::html_escape(s)
}

fn escape_md_inline(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn render_markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

// ---------------------------------------------------------------------------
// Static assets
// ---------------------------------------------------------------------------

const STYLE_CSS: &str = include_str!("../templates/style.css");
const SEARCH_JS: &str = include_str!("../templates/search.js");
