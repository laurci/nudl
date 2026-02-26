use nudl_ast::ast::*;

pub(super) fn fmt_ast(module: &Module) -> String {
    let mut out = String::new();
    out.push_str("Module:\n");
    for item in &module.items {
        fmt_ast_item(&item.node, &mut out, 1);
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn fmt_ast_item(item: &Item, out: &mut String, level: usize) {
    match item {
        Item::FnDef {
            name,
            params,
            return_type,
            body,
            is_pub,
        } => {
            indent(out, level);
            if *is_pub {
                out.push_str("pub ");
            }
            out.push_str(&format!("FnDef \"{}\" (", name));
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let mutstr = if p.is_mut { "mut " } else { "" };
                out.push_str(&format!(
                    "{}{}: {}",
                    mutstr,
                    p.name,
                    fmt_type_expr(&p.ty.node)
                ));
            }
            out.push_str(&format!(
                ") -> {}:\n",
                return_type
                    .as_ref()
                    .map(|t| fmt_type_expr(&t.node))
                    .unwrap_or_else(|| "()".into())
            ));
            fmt_ast_block(&body.node, out, level + 1);
        }
        Item::StructDef {
            name,
            fields,
            is_pub,
        } => {
            indent(out, level);
            if *is_pub {
                out.push_str("pub ");
            }
            out.push_str(&format!("StructDef \"{}\":\n", name));
            for f in fields {
                indent(out, level + 1);
                out.push_str(&format!("{}: {}\n", f.name, fmt_type_expr(&f.ty.node)));
            }
        }
        Item::ExternBlock { library, items } => {
            indent(out, level);
            out.push_str("ExternBlock");
            if let Some(lib) = library {
                out.push_str(&format!(" \"{}\"", lib));
            }
            out.push_str(":\n");
            for item in items {
                indent(out, level + 1);
                let decl = &item.node;
                out.push_str(&format!("ExternFn \"{}\" (", decl.name));
                for (i, p) in decl.params.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("{}: {}", p.name, fmt_type_expr(&p.ty.node)));
                }
                out.push_str(&format!(
                    ") -> {}\n",
                    decl.return_type
                        .as_ref()
                        .map(|t| fmt_type_expr(&t.node))
                        .unwrap_or_else(|| "()".into())
                ));
            }
        }
        Item::ImplBlock { type_name, methods } => {
            indent(out, level);
            out.push_str(&format!("ImplBlock \"{}\":\n", type_name));
            for method in methods {
                fmt_ast_item(&method.node, out, level + 1);
            }
        }
    }
}

fn fmt_ast_block(block: &Block, out: &mut String, level: usize) {
    indent(out, level);
    out.push_str("Block:\n");
    for stmt in &block.stmts {
        fmt_ast_stmt(&stmt.node, out, level + 1);
    }
    if let Some(ref tail) = block.tail_expr {
        indent(out, level + 1);
        out.push_str("Tail: ");
        fmt_ast_expr(&tail.node, out, level + 1);
        out.push('\n');
    }
}

fn fmt_ast_stmt(stmt: &Stmt, out: &mut String, level: usize) {
    match stmt {
        Stmt::Expr(expr) => {
            indent(out, level);
            out.push_str("Expr: ");
            fmt_ast_expr(&expr.node, out, level);
            out.push('\n');
        }
        Stmt::Let {
            name,
            ty,
            value,
            is_mut,
        } => {
            indent(out, level);
            out.push_str("Let ");
            if *is_mut {
                out.push_str("mut ");
            }
            out.push_str(name);
            if let Some(t) = ty {
                out.push_str(&format!(": {}", fmt_type_expr(&t.node)));
            }
            out.push_str(" = ");
            fmt_ast_expr(&value.node, out, level);
            out.push('\n');
        }
        Stmt::Const { name, ty, value } => {
            indent(out, level);
            out.push_str("Const ");
            out.push_str(name);
            if let Some(t) = ty {
                out.push_str(&format!(": {}", fmt_type_expr(&t.node)));
            }
            out.push_str(" = ");
            fmt_ast_expr(&value.node, out, level);
            out.push('\n');
        }
        Stmt::Item(item) => {
            fmt_ast_item(&item.node, out, level);
        }
    }
}

fn fmt_ast_expr(expr: &Expr, out: &mut String, level: usize) {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::String(s) => out.push_str(&format!("Literal(String {:?})", s)),
            Literal::Int(s, suffix) => {
                out.push_str(&format!("Literal(Int {})", s));
                if let Some(suf) = suffix {
                    out.push_str(&format!("{:?}", suf));
                }
            }
            Literal::Float(s) => out.push_str(&format!("Literal(Float {})", s)),
            Literal::Bool(b) => out.push_str(&format!("Literal(Bool {})", b)),
            Literal::Char(c) => out.push_str(&format!("Literal(Char {:?})", c)),
            Literal::TemplateString { parts, exprs } => {
                out.push_str("TemplateString(");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("{:?}", part));
                    if i < exprs.len() {
                        out.push_str(", ");
                        fmt_ast_expr(&exprs[i].node, out, level);
                    }
                }
                out.push(')');
            }
        },
        Expr::Ident(name) => {
            out.push_str(&format!("Ident \"{}\"", name));
        }
        Expr::Call { callee, args } => {
            out.push_str("Call ");
            fmt_ast_expr(&callee.node, out, level);
            out.push('\n');
            for arg in args {
                indent(out, level + 1);
                out.push_str("Arg: ");
                fmt_ast_expr(&arg.value.node, out, level + 1);
                out.push('\n');
            }
        }
        Expr::Block(block) => {
            out.push_str("Block\n");
            fmt_ast_block(block, out, level + 1);
        }
        Expr::Return(val) => {
            out.push_str("Return");
            if let Some(v) = val {
                out.push(' ');
                fmt_ast_expr(&v.node, out, level);
            }
        }
        Expr::Binary { op, left, right } => {
            out.push_str(&format!("Binary({:?}, ", op));
            fmt_ast_expr(&left.node, out, level);
            out.push_str(", ");
            fmt_ast_expr(&right.node, out, level);
            out.push(')');
        }
        Expr::Unary { op, operand } => {
            out.push_str(&format!("Unary({:?}, ", op));
            fmt_ast_expr(&operand.node, out, level);
            out.push(')');
        }
        Expr::Assign { target, value } => {
            out.push_str("Assign(");
            fmt_ast_expr(&target.node, out, level);
            out.push_str(" = ");
            fmt_ast_expr(&value.node, out, level);
            out.push(')');
        }
        Expr::CompoundAssign { op, target, value } => {
            out.push_str(&format!("CompoundAssign({:?}, ", op));
            fmt_ast_expr(&target.node, out, level);
            out.push_str(", ");
            fmt_ast_expr(&value.node, out, level);
            out.push(')');
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            out.push_str("If ");
            fmt_ast_expr(&condition.node, out, level);
            out.push('\n');
            fmt_ast_block(&then_branch.node, out, level + 1);
            if let Some(else_br) = else_branch {
                indent(out, level + 1);
                out.push_str("Else: ");
                fmt_ast_expr(&else_br.node, out, level + 1);
                out.push('\n');
            }
        }
        Expr::Cast { expr, target_type } => {
            fmt_ast_expr(&expr.node, out, level);
            out.push_str(&format!(" as {}", fmt_type_expr(&target_type.node)));
        }
        Expr::While {
            condition,
            body,
            label,
        } => {
            if let Some(l) = label {
                out.push_str(&format!("'{}: ", l));
            }
            out.push_str("While ");
            fmt_ast_expr(&condition.node, out, level);
            out.push('\n');
            fmt_ast_block(&body.node, out, level + 1);
        }
        Expr::Loop { body, label } => {
            if let Some(l) = label {
                out.push_str(&format!("'{}: ", l));
            }
            out.push_str("Loop\n");
            fmt_ast_block(&body.node, out, level + 1);
        }
        Expr::Break { label, value } => {
            out.push_str("Break");
            if let Some(l) = label {
                out.push_str(&format!(" '{}", l));
            }
            if let Some(v) = value {
                out.push(' ');
                fmt_ast_expr(&v.node, out, level);
            }
        }
        Expr::Continue { label } => {
            out.push_str("Continue");
            if let Some(l) = label {
                out.push_str(&format!(" '{}", l));
            }
        }
        Expr::Grouped(inner) => {
            out.push_str("Grouped(");
            fmt_ast_expr(&inner.node, out, level);
            out.push(')');
        }
        Expr::StructLiteral { name, fields } => {
            out.push_str(&format!("StructLiteral \"{}\" {{\n", name));
            for (fname, fval) in fields {
                indent(out, level + 1);
                out.push_str(&format!("{}: ", fname));
                fmt_ast_expr(&fval.node, out, level + 1);
                out.push('\n');
            }
            indent(out, level);
            out.push('}');
        }
        Expr::FieldAccess { object, field } => {
            fmt_ast_expr(&object.node, out, level);
            out.push_str(&format!(".{}", field));
        }
        Expr::TupleLiteral(elements) => {
            out.push_str("Tuple(");
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                fmt_ast_expr(&elem.node, out, level);
            }
            out.push(')');
        }
        Expr::ArrayLiteral(elements) => {
            out.push('[');
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                fmt_ast_expr(&elem.node, out, level);
            }
            out.push(']');
        }
        Expr::ArrayRepeat { value, count } => {
            out.push('[');
            fmt_ast_expr(&value.node, out, level);
            out.push_str(&format!("; {}]", count));
        }
        Expr::IndexAccess { object, index } => {
            fmt_ast_expr(&object.node, out, level);
            out.push('[');
            fmt_ast_expr(&index.node, out, level);
            out.push(']');
        }
        Expr::Range {
            start,
            end,
            inclusive,
        } => {
            fmt_ast_expr(&start.node, out, level);
            if *inclusive {
                out.push_str("..=");
            } else {
                out.push_str("..");
            }
            fmt_ast_expr(&end.node, out, level);
        }
        Expr::For {
            binding,
            iter,
            body,
        } => {
            out.push_str(&format!("For {} in ", binding));
            fmt_ast_expr(&iter.node, out, level);
            out.push('\n');
            fmt_ast_block(&body.node, out, level + 1);
        }
        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            fmt_ast_expr(&object.node, out, level);
            out.push_str(&format!(".{}(\n", method));
            for arg in args {
                indent(out, level + 1);
                if let Some(name) = &arg.name {
                    out.push_str(&format!("{}: ", name));
                }
                fmt_ast_expr(&arg.value.node, out, level + 1);
                out.push('\n');
            }
            indent(out, level);
            out.push(')');
        }
        Expr::StaticCall {
            type_name,
            method,
            args,
        } => {
            out.push_str(&format!("{}::{}(\n", type_name, method));
            for arg in args {
                indent(out, level + 1);
                if let Some(name) = &arg.name {
                    out.push_str(&format!("{}: ", name));
                }
                fmt_ast_expr(&arg.value.node, out, level + 1);
                out.push('\n');
            }
            indent(out, level);
            out.push(')');
        }
    }
}

fn fmt_type_expr(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Unit => "()".into(),
        TypeExpr::Tuple(elements) => {
            let parts: Vec<String> = elements.iter().map(|e| fmt_type_expr(&e.node)).collect();
            format!("({})", parts.join(", "))
        }
        TypeExpr::FixedArray { element, length } => {
            format!("[{}; {}]", fmt_type_expr(&element.node), length)
        }
    }
}
