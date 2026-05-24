//! SIR text dump (read-only). Used by `mty dump --sir` and by
//! snapshot tests.

use crate::ir::*;
use std::fmt::Write;

pub fn dump_program(p: &Program) -> String {
    let mut out = String::new();
    out.push_str("// Mighty IR (SIR) v0.6\n");
    if !p.errors.is_empty() {
        out.push_str("// lowering errors:\n");
        for e in &p.errors {
            let _ = writeln!(out, "//   - {}", e);
        }
    }
    if !p.adts.is_empty() {
        for a in &p.adts {
            dump_adt(a, &mut out);
        }
        out.push('\n');
    }
    if !p.agents.is_empty() {
        for ag in &p.agents {
            dump_agent(ag, &mut out);
        }
        out.push('\n');
    }
    for f in &p.fns {
        dump_fn(f, &mut out);
        out.push('\n');
    }
    out
}

fn dump_adt(a: &AdtRef, out: &mut String) {
    let kind = match a.kind {
        AdtRefKind::Struct => "struct",
        AdtRefKind::Enum => "enum",
        AdtRefKind::Opaque => "opaque",
    };
    let _ = writeln!(out, "{} {} {{", kind, a.name);
    for v in &a.variants {
        let fields: Vec<String> = v
            .fields
            .iter()
            .map(|f| match &f.name {
                Some(n) => format!("{}: {}", n, dump_ty(&f.ty)),
                None => dump_ty(&f.ty),
            })
            .collect();
        let _ = writeln!(out, "  {}({})", v.name, fields.join(", "));
    }
    let _ = writeln!(out, "}}");
}

fn dump_agent(a: &Agent, out: &mut String) {
    let _ = writeln!(
        out,
        "agent {} {{ ctor = fn{}, handlers = [{}] }}",
        a.name,
        a.ctor.0,
        a.handlers
            .iter()
            .map(|(m, f)| format!("{} -> fn{}", m, f.0))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

pub fn dump_fn(f: &Function, out: &mut String) {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let d = &f.locals[p.0 as usize];
            format!("_{}: {}", p.0, dump_ty(&d.ty))
        })
        .collect();
    let _ = writeln!(
        out,
        "fn{}: {}({}) -> {} {{",
        f.id.0,
        f.name,
        params.join(", "),
        dump_ty(&f.ret_ty)
    );
    // Declare non-param locals up front, MIR-style.
    let param_count = f.params.len();
    for (idx, decl) in f.locals.iter().enumerate() {
        // Skip parameter locals (already shown in signature) but always
        // show the return slot.
        if idx > 0 && idx <= param_count {
            continue;
        }
        let mut_marker = if decl.mutable { "mut " } else { "" };
        let name = if decl.name.is_empty() {
            String::new()
        } else {
            format!(" /* {} */", decl.name)
        };
        let _ = writeln!(
            out,
            "  let {}_{}: {}{}",
            mut_marker,
            idx,
            dump_ty(&decl.ty),
            name
        );
    }
    out.push('\n');
    for blk in &f.blocks {
        let _ = writeln!(out, "  bb{}:", blk.id.0);
        for s in &blk.stmts {
            let _ = writeln!(out, "    {}", dump_stmt(s));
        }
        let _ = writeln!(out, "    {}", dump_term(&blk.terminator));
        out.push('\n');
    }
    out.push_str("}\n");
}

fn dump_stmt(s: &Stmt) -> String {
    match s {
        Stmt::Assign(p, r) => format!("{} := {}", dump_place(p), dump_rvalue(r)),
        Stmt::Drop(l) => format!("drop _{}", l.0),
        Stmt::StorageLive(l) => format!("StorageLive _{}", l.0),
        Stmt::StorageDead(l) => format!("StorageDead _{}", l.0),
        Stmt::ArenaPush(a) => format!("ArenaPush(arena{})", a.0),
        Stmt::ArenaPop(a) => format!("ArenaPop(arena{})", a.0),
        Stmt::EffectInvoke {
            effect,
            op,
            args,
            out,
        } => {
            let args_str: Vec<String> = args.iter().map(dump_operand).collect();
            let lhs = match out {
                Some(p) => format!("{} := ", dump_place(p)),
                None => String::new(),
            };
            format!(
                "{}effect[{}]({}, {})",
                lhs,
                effect.0,
                dump_effect_op(op),
                args_str.join(", ")
            )
        }
        Stmt::Nop => "nop".into(),
    }
}

fn dump_effect_op(op: &EffectOp) -> String {
    match op {
        EffectOp::GenericCall { path, method } => {
            format!("{}.{}", path.join("."), method)
        }
    }
}

fn dump_term(t: &Term) -> String {
    match t {
        Term::Goto(b) => format!("goto bb{}", b.0),
        Term::If { cond, then, else_ } => format!(
            "if {} {{ goto bb{} }} else {{ goto bb{} }}",
            dump_operand(cond),
            then.0,
            else_.0
        ),
        Term::SwitchInt {
            discr,
            arms,
            default,
        } => {
            let arms: Vec<String> = arms
                .iter()
                .map(|(v, b)| format!("{} => bb{}", v, b.0))
                .collect();
            format!(
                "switch_int {} {{ {}, _ => bb{} }}",
                dump_operand(discr),
                arms.join(", "),
                default.0
            )
        }
        Term::SwitchVariant {
            discr,
            adt: _,
            arms,
            default,
        } => {
            let arms: Vec<String> = arms
                .iter()
                .map(|(v, b)| format!("variant {} => bb{}", v, b.0))
                .collect();
            format!(
                "switch_variant {} {{ {}, _ => bb{} }}",
                dump_operand(discr),
                arms.join(", "),
                default.0
            )
        }
        Term::Return(o) => format!("return {}", dump_operand(o)),
        Term::Panic { msg } => format!("panic {}", dump_operand(msg)),
        Term::Unreachable => "unreachable".into(),
        Term::TryReturnErr(o) => format!("try_return_err {}", dump_operand(o)),
        Term::Suspend { resume } => format!("suspend resume = bb{}", resume.0),
    }
}

fn dump_operand(o: &Operand) -> String {
    match o {
        Operand::Copy(p) => format!("copy {}", dump_place(p)),
        Operand::Move(p) => format!("move {}", dump_place(p)),
        Operand::Const(c) => dump_const(c),
    }
}

fn dump_place(p: &Place) -> String {
    let mut s = format!("_{}", p.local.0);
    for proj in &p.proj {
        match proj {
            Projection::Field(i) => s.push_str(&format!(".f{}", i)),
            Projection::TupleIndex(i) => s.push_str(&format!(".{}", i)),
            Projection::Deref => s.insert(0, '*'),
            Projection::Index(l) => s.push_str(&format!("[_{}]", l.0)),
            Projection::VariantField(v, f) => s.push_str(&format!(".v{}.f{}", v, f)),
        }
    }
    s
}

fn dump_rvalue(r: &Rvalue) -> String {
    match r {
        Rvalue::Use(o) => dump_operand(o),
        Rvalue::Const(c) => format!("const {}", dump_const(c)),
        Rvalue::BinOp(op, l, r) => {
            format!(
                "{} {} {}",
                dump_operand(l),
                dump_binop(*op),
                dump_operand(r)
            )
        }
        Rvalue::UnOp(op, x) => format!("{}{}", dump_unop(*op), dump_operand(x)),
        Rvalue::Ref { mutable, place } => {
            let m = if *mutable { "mut " } else { "" };
            format!("&{}{}", m, dump_place(place))
        }
        Rvalue::Deref(o) => format!("*{}", dump_operand(o)),
        Rvalue::AdtInit {
            adt,
            variant,
            fields,
        } => {
            let parts: Vec<String> = fields.iter().map(dump_operand).collect();
            format!("Adt{}::V{}({})", adt.0, variant, parts.join(", "))
        }
        Rvalue::TupleInit(xs) => {
            let parts: Vec<String> = xs.iter().map(dump_operand).collect();
            format!("({})", parts.join(", "))
        }
        Rvalue::ArrayInit(xs) => {
            let parts: Vec<String> = xs.iter().map(dump_operand).collect();
            format!("[{}]", parts.join(", "))
        }
        Rvalue::FieldRead { receiver, field } => format!("{}.f{}", dump_place(receiver), field),
        Rvalue::TupleRead { receiver, idx } => format!("{}.{}", dump_place(receiver), idx),
        Rvalue::IndexRead { receiver, index } => {
            format!("{}[{}]", dump_place(receiver), dump_operand(index))
        }
        Rvalue::Call { func, args } => {
            let parts: Vec<String> = args.iter().map(dump_operand).collect();
            format!("call {}({})", dump_fnref(func), parts.join(", "))
        }
        Rvalue::MethodCall {
            receiver,
            method,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(dump_operand).collect();
            format!(
                "({}).{}({})",
                dump_operand(receiver),
                method,
                parts.join(", ")
            )
        }
        Rvalue::AgentSpawn { agent, args } => {
            let parts: Vec<String> = args.iter().map(dump_operand).collect();
            format!("spawn agent{}({})", agent.0, parts.join(", "))
        }
        Rvalue::Send { target, msg, args } => {
            let parts: Vec<String> = args.iter().map(dump_operand).collect();
            format!("({})!{}({})", dump_operand(target), msg, parts.join(", "))
        }
        Rvalue::Ask {
            target,
            msg,
            args,
            deadline_ms,
        } => {
            let parts: Vec<String> = args.iter().map(dump_operand).collect();
            let dl = match deadline_ms {
                Some(d) => format!(" @{}ms", d),
                None => String::new(),
            };
            format!(
                "({})?{}({}){}",
                dump_operand(target),
                msg,
                parts.join(", "),
                dl
            )
        }
        Rvalue::CapValue { family, constraint } => format!("cap({:?}, {:?})", family, constraint),
        Rvalue::Cast { src, ty } => format!("cast {} as {}", dump_operand(src), dump_ty(ty)),
    }
}

fn dump_fnref(f: &FnRef) -> String {
    match f {
        FnRef::User(id) => format!("fn{}", id.0),
        FnRef::Builtin(b) => format!("@{}", dump_builtin(b)),
    }
}

fn dump_builtin(b: &BuiltinId) -> String {
    match b {
        BuiltinId::Log => "log".into(),
        BuiltinId::Print => "print".into(),
        BuiltinId::Panic => "panic".into(),
        BuiltinId::Spawn => "spawn".into(),
        BuiltinId::Move => "move".into(),
        BuiltinId::Fetch => "fetch".into(),
        BuiltinId::RawPtr => "raw_ptr".into(),
        BuiltinId::Valid => "valid".into(),
        BuiltinId::Null => "null".into(),
        BuiltinId::Extern(n) => format!("extern:{}", n),
        BuiltinId::DomOp(op) => format!("dom.{}", op),
    }
}

fn dump_binop(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Rem => "%",
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        Shl => "<<",
        Shr => ">>",
        Eq => "==",
        Ne => "!=",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        And => "&&",
        Or => "||",
    }
}

fn dump_unop(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    }
}

fn dump_const(c: &Const) -> String {
    match c {
        Const::Unit => "()".into(),
        Const::Bool(b) => format!("{}", b),
        Const::Int(v, _) => format!("{}", v),
        Const::Float(v, _) => format!("{}", v),
        Const::Str(s) => format!("{:?}", s),
        Const::Char(c) => format!("'{}'", c),
        Const::Duration { value, unit } => format!("{}{}", value, unit),
        Const::Size { value, unit } => format!("{}{}", value, unit),
        Const::FnPtr(f) => format!("&{}", dump_fnref(f)),
        Const::NullPtr => "null".into(),
    }
}

pub fn dump_ty(t: &IrTy) -> String {
    match t {
        IrTy::Bool => "Bool".into(),
        IrTy::Int(k) => format!("{:?}", k),
        IrTy::Float(k) => format!("{:?}", k),
        IrTy::Char => "Char".into(),
        IrTy::Str => "Str".into(),
        IrTy::String => "String".into(),
        IrTy::Bytes => "Bytes".into(),
        IrTy::Unit => "Unit".into(),
        IrTy::Never => "Never".into(),
        IrTy::Duration => "Duration".into(),
        IrTy::Size => "Size".into(),
        IrTy::Tuple(xs) => {
            let parts: Vec<String> = xs.iter().map(dump_ty).collect();
            format!("({})", parts.join(", "))
        }
        IrTy::Array { elem, len } => match len {
            Some(n) => format!("[{}; {}]", dump_ty(elem), n),
            None => format!("[{}]", dump_ty(elem)),
        },
        IrTy::Ref { mutable, inner } => {
            let m = if *mutable { "mut " } else { "" };
            format!("&{}{}", m, dump_ty(inner))
        }
        IrTy::Fn { params, ret } => {
            let parts: Vec<String> = params.iter().map(dump_ty).collect();
            format!("fn({}) -> {}", parts.join(", "), dump_ty(ret))
        }
        IrTy::Adt(id, args) => {
            if args.is_empty() {
                format!("Adt{}", id.0)
            } else {
                let parts: Vec<String> = args.iter().map(dump_ty).collect();
                format!("Adt{}[{}]", id.0, parts.join(", "))
            }
        }
        IrTy::Cap {
            family,
            constraint: _,
        } => format!("Cap({:?})", family),
        IrTy::Dyn(n) => format!("dyn {}", n),
        IrTy::RawPtr(inner) => format!("*{}", dump_ty(inner)),
        IrTy::Module(n) => format!("module {}", n),
        IrTy::Param(n) => n.clone(),
        IrTy::Error => "{error}".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_types::IntKind;

    #[test]
    fn empty_program_dumps_header() {
        let p = Program::default();
        let s = dump_program(&p);
        assert!(s.contains("Mighty IR (SIR) v0.6"));
    }

    #[test]
    fn const_int_dumps() {
        assert_eq!(dump_const(&Const::Int(42, IntKind::I32)), "42");
        assert_eq!(dump_const(&Const::Bool(true)), "true");
        assert_eq!(dump_const(&Const::Str("x".into())), "\"x\"");
    }
}
