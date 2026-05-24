//! HIR S-expression dump — for snapshot testing.

use crate::{HirBlock, HirExpr, HirLiteral, HirStmt, HirType, Item, Package};
use std::fmt::Write;

pub fn dump_package(pkg: &Package) -> String {
    let mut out = String::new();
    writeln!(out, "(package").unwrap();
    for &item_id in &pkg.top_level {
        dump_item(&mut out, pkg, &pkg.items[item_id], 1);
    }
    writeln!(out, ")").unwrap();
    out
}

fn ind(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

fn dump_item(out: &mut String, pkg: &Package, item: &Item, depth: usize) {
    match item {
        Item::Fn(id) => {
            let f = &pkg.fns[*id];
            ind(out, depth);
            writeln!(
                out,
                "(fn {} ({})",
                f.name,
                f.params
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
            if let Some(b) = f.body {
                dump_block(out, pkg, &pkg.blocks[b], depth + 1);
            }
            ind(out, depth);
            writeln!(out, ")").unwrap();
        }
        Item::Agent(id) => {
            let a = &pkg.agents[*id];
            ind(out, depth);
            writeln!(
                out,
                "(agent {} ctor=({}) protocols=({})",
                a.name,
                a.ctor_params.join(" "),
                a.protocols
                    .iter()
                    .map(|t| dump_type(pkg, &pkg.types[*t]))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
            for s in &a.state {
                ind(out, depth + 1);
                writeln!(out, "(state {})", s.name).unwrap();
            }
            for h in &a.handlers {
                ind(out, depth + 1);
                writeln!(out, "(on {} ({})", h.message, h.params.join(" ")).unwrap();
                dump_block(out, pkg, &pkg.blocks[h.body], depth + 2);
                ind(out, depth + 1);
                writeln!(out, ")").unwrap();
            }
            ind(out, depth);
            writeln!(out, ")").unwrap();
        }
        Item::Protocol(id) => {
            let p = &pkg.protocols[*id];
            ind(out, depth);
            writeln!(
                out,
                "(protocol {} msgs=({}))",
                p.name,
                p.messages
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
        Item::Struct(id) => {
            let s = &pkg.structs[*id];
            ind(out, depth);
            writeln!(
                out,
                "(struct {} fields=({}))",
                s.name,
                s.fields
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
        Item::Enum(id) => {
            let e = &pkg.enums[*id];
            ind(out, depth);
            writeln!(
                out,
                "(enum {} variants=({}))",
                e.name,
                e.variants
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
        Item::TypeAlias(id) => {
            let t = &pkg.type_aliases[*id];
            ind(out, depth);
            writeln!(
                out,
                "(type-alias {} {})",
                t.name,
                dump_type(pkg, &pkg.types[t.ty])
            )
            .unwrap();
        }
        Item::Supervisor(id) => {
            let s = &pkg.supervisors[*id];
            ind(out, depth);
            writeln!(
                out,
                "(supervisor {} strategy={} children=({}))",
                s.name,
                s.strategy,
                s.children
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
        Item::Use(u) => {
            ind(out, depth);
            writeln!(out, "(use {})", u.path.join(".")).unwrap();
        }
        Item::Mod(m) => {
            ind(out, depth);
            writeln!(out, "(mod {})", m.path.join(".")).unwrap();
        }
        Item::ExternBlock(_)
        | Item::ExportDecl(_)
        | Item::Macro(_)
        | Item::Impl(_)
        | Item::Trait(_)
        | Item::Const(_)
        | Item::Sandbox(_) => {
            ind(out, depth);
            writeln!(out, "(item ...)").unwrap();
        }
    }
}

fn dump_block(out: &mut String, pkg: &Package, b: &HirBlock, depth: usize) {
    for s in &b.stmts {
        ind(out, depth);
        match s {
            HirStmt::Let { .. } => writeln!(out, "(let ...)").unwrap(),
            HirStmt::Expr(e) => {
                writeln!(out, "{}", dump_expr(pkg, &pkg.exprs[*e])).unwrap();
            }
        }
    }
    if let Some(t) = b.tail {
        ind(out, depth);
        writeln!(out, "{}", dump_expr(pkg, &pkg.exprs[t])).unwrap();
    }
}

fn dump_expr(pkg: &Package, e: &HirExpr) -> String {
    match e {
        HirExpr::Literal(l) => dump_lit(l),
        HirExpr::Path(p) => p.join("."),
        HirExpr::Call { callee, args } => format!(
            "(call {} ({}))",
            dump_expr(pkg, &pkg.exprs[*callee]),
            args.iter()
                .map(|a| dump_expr(pkg, &pkg.exprs[a.value]))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        HirExpr::Binary { op, lhs, rhs } => format!(
            "({:?} {} {})",
            op,
            dump_expr(pkg, &pkg.exprs[*lhs]),
            dump_expr(pkg, &pkg.exprs[*rhs])
        ),
        HirExpr::Send { target, msg, args } => format!(
            "(send {} !{} ({}))",
            dump_expr(pkg, &pkg.exprs[*target]),
            msg,
            args.iter()
                .map(|a| dump_expr(pkg, &pkg.exprs[a.value]))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        HirExpr::Ask { target, msg, args } => format!(
            "(ask {} ?{} ({}))",
            dump_expr(pkg, &pkg.exprs[*target]),
            msg,
            args.iter()
                .map(|a| dump_expr(pkg, &pkg.exprs[a.value]))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        HirExpr::Deadline { inner, dur } => format!(
            "(deadline {} @{})",
            dump_expr(pkg, &pkg.exprs[*inner]),
            dump_expr(pkg, &pkg.exprs[*dur])
        ),
        HirExpr::Arena { name, body } => {
            format!("(arena {} {})", name, dump_expr(pkg, &pkg.exprs[*body]))
        }
        _ => "(expr ...)".into(),
    }
}

fn dump_type(pkg: &Package, t: &HirType) -> String {
    match t {
        HirType::Path { segments, generics } => {
            let segs = segments.join(".");
            if generics.is_empty() {
                segs
            } else {
                format!(
                    "{}[{}]",
                    segs,
                    generics
                        .iter()
                        .map(|g| dump_type(pkg, &pkg.types[*g]))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
        HirType::Result { ok, err } => format!(
            "{}!{}",
            dump_type(pkg, &pkg.types[*ok]),
            dump_type(pkg, &pkg.types[*err])
        ),
        HirType::Borrow { mutable, inner } => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            dump_type(pkg, &pkg.types[*inner])
        ),
        HirType::Unit => "()".into(),
        HirType::Unknown => "?".into(),
        _ => "(ty ...)".into(),
    }
}

fn dump_lit(l: &HirLiteral) -> String {
    match l {
        HirLiteral::Int(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
        HirLiteral::Float(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
        HirLiteral::Str(s) => format!("{:?}", s),
        HirLiteral::Char(c) => format!("'{}'", c),
        HirLiteral::Bool(b) => b.to_string(),
        HirLiteral::Duration { value, unit } => format!("{}{}", value, unit),
        HirLiteral::Size { value, unit } => format!("{}{}", value, unit),
    }
}
