use std::path::Path;

use nudl_ast::ast::*;
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_backend_llvm::codegen;
use nudl_bc::checker::Checker;
use nudl_bc::ir::*;
use nudl_bc::lower::Lowerer;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::source::SourceMap;
use nudl_vm::Vm;

#[derive(Default)]
pub struct DumpOptions {
    pub dump_ast: bool,
    pub dump_ir: bool,
    pub dump_asm: bool,
    pub dump_llvm_ir: bool,
}

pub struct PipelineResult {
    pub source_map: SourceMap,
    pub diagnostics: DiagnosticBag,
}

pub fn check(source_path: &Path, dump: &DumpOptions) -> PipelineResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return PipelineResult {
                source_map,
                diagnostics,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return PipelineResult {
            source_map,
            diagnostics,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return PipelineResult {
            source_map,
            diagnostics,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);

    if dump.dump_ir && !diagnostics.has_errors() {
        let program = Lowerer::new(checked).lower(&module);
        eprintln!("{}", fmt_ir(&program));
    }

    PipelineResult {
        source_map,
        diagnostics,
    }
}

pub struct CompileResult {
    pub source_map: SourceMap,
    pub diagnostics: DiagnosticBag,
    pub success: bool,
}

pub fn build(source_path: &Path, output_path: &Path, release: bool, dump: &DumpOptions) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let mut program = Lowerer::new(checked).lower(&module);
    program.source_map = Some(source_map);

    if dump.dump_ir {
        eprintln!("{}", fmt_ir(&program));
    }

    if dump.dump_llvm_ir {
        match codegen::compile_to_llvm_ir(&program) {
            Ok(ir) => eprintln!("{}", ir),
            Err(e) => eprintln!("error generating LLVM IR: {}", e),
        }
    }

    if dump.dump_asm {
        match codegen::compile_to_asm_text(&program, release) {
            Ok(asm) => eprintln!("{}", asm),
            Err(e) => eprintln!("error generating assembly: {}", e),
        }
    }

    let result = codegen::compile_to_executable(&program, output_path, release);
    let source_map = program.source_map.unwrap_or_default();
    match result {
        Ok(()) => CompileResult {
            source_map,
            diagnostics,
            success: true,
        },
        Err(e) => {
            eprintln!("error: {}", e);
            CompileResult {
                source_map,
                diagnostics,
                success: false,
            }
        }
    }
}

pub fn run_vm(source_path: &Path, dump: &DumpOptions) -> CompileResult {
    let mut source_map = SourceMap::new();
    let mut diagnostics = DiagnosticBag::new();

    let content = match std::fs::read_to_string(source_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", source_path.display(), e);
            return CompileResult {
                source_map,
                diagnostics,
                success: false,
            };
        }
    };

    let file_id = source_map.add_file(source_path.to_path_buf(), content.clone());

    let (tokens, lex_diags) = Lexer::new(&content, file_id).tokenize();
    diagnostics.merge(lex_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let (module, parse_diags) = Parser::new(tokens).parse_module();
    diagnostics.merge(parse_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    if dump.dump_ast {
        eprintln!("{}", fmt_ast(&module));
    }

    let (checked, check_diags) = Checker::new().check(&module);
    diagnostics.merge(check_diags);
    if diagnostics.has_errors() {
        return CompileResult {
            source_map,
            diagnostics,
            success: false,
        };
    }

    let program = Lowerer::new(checked).lower(&module);

    if dump.dump_ir {
        eprintln!("{}", fmt_ir(&program));
    }

    let mut vm = Vm::new();
    match vm.run(&program) {
        Ok(_) => CompileResult {
            source_map,
            diagnostics,
            success: true,
        },
        Err(e) => {
            eprintln!("vm error: {}", e);
            CompileResult {
                source_map,
                diagnostics,
                success: false,
            }
        }
    }
}

// --- AST pretty-printer ---

fn fmt_ast(module: &Module) -> String {
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
                out.push_str(&format!("{}: {}", p.name, fmt_type_expr(&p.ty.node)));
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
        Expr::While { condition, body } => {
            out.push_str("While ");
            fmt_ast_expr(&condition.node, out, level);
            out.push('\n');
            fmt_ast_block(&body.node, out, level + 1);
        }
        Expr::Loop { body } => {
            out.push_str("Loop\n");
            fmt_ast_block(&body.node, out, level + 1);
        }
        Expr::Break(val) => {
            out.push_str("Break");
            if let Some(v) = val {
                out.push(' ');
                fmt_ast_expr(&v.node, out, level);
            }
        }
        Expr::Continue => {
            out.push_str("Continue");
        }
        Expr::Grouped(inner) => {
            out.push_str("Grouped(");
            fmt_ast_expr(&inner.node, out, level);
            out.push(')');
        }
    }
}

fn fmt_type_expr(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Unit => "()".into(),
    }
}

// --- IR pretty-printer ---

fn fmt_ir(program: &Program) -> String {
    let mut out = String::new();

    // String constants
    out.push_str("string_constants:\n");
    for (i, s) in program.string_constants.iter().enumerate() {
        out.push_str(&format!("  [{}] {:?}\n", i, s));
    }
    out.push('\n');

    // Functions
    for func in &program.functions {
        let func_name = program.interner.resolve(func.name);
        if func.is_extern {
            out.push_str(&format!("function {}(", func_name));
            for (i, (pname, _pty)) in func.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("{}", program.interner.resolve(*pname)));
            }
            out.push(')');
            if let Some(ref ext_sym) = func.extern_symbol {
                out.push_str(&format!("  [extern: {}]", ext_sym));
            }
            out.push('\n');
        } else {
            out.push_str(&format!("function {}(", func_name));
            for (i, (_pname, _pty)) in func.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", i));
            }
            out.push_str("):\n");

            for block in &func.blocks {
                out.push_str(&format!("  block b{}:\n", block.id.0));
                for inst in &block.instructions {
                    out.push_str("    ");
                    fmt_instruction(inst, &mut out, &program.interner);
                    out.push('\n');
                }
                out.push_str("    ");
                fmt_terminator(&block.terminator, &mut out);
                out.push('\n');
            }
        }
        out.push('\n');
    }

    out
}

fn fmt_instruction(
    inst: &Instruction,
    out: &mut String,
    interner: &nudl_core::intern::StringInterner,
) {
    match inst {
        Instruction::Const(reg, val) => {
            out.push_str(&format!("r{} = Const(", reg.0));
            match val {
                ConstValue::Unit => out.push_str("Unit"),
                ConstValue::I32(v) => out.push_str(&format!("I32({})", v)),
                ConstValue::I64(v) => out.push_str(&format!("I64({})", v)),
                ConstValue::U64(v) => out.push_str(&format!("U64({})", v)),
                ConstValue::Bool(v) => out.push_str(&format!("Bool({})", v)),
                ConstValue::F64(v) => out.push_str(&format!("F64({})", v)),
                ConstValue::Char(v) => out.push_str(&format!("Char({:?})", v)),
                ConstValue::StringLiteral(idx) => out.push_str(&format!("StringLiteral({})", idx)),
            }
            out.push(')');
        }
        Instruction::ConstUnit(reg) => {
            out.push_str(&format!("r{} = ConstUnit", reg.0));
        }
        Instruction::StringPtr(dst, src) => {
            out.push_str(&format!("r{} = StringPtr(r{})", dst.0, src.0));
        }
        Instruction::StringLen(dst, src) => {
            out.push_str(&format!("r{} = StringLen(r{})", dst.0, src.0));
        }
        Instruction::StringConstPtr(reg, idx) => {
            out.push_str(&format!("r{} = StringConstPtr({})", reg.0, idx));
        }
        Instruction::StringConstLen(reg, idx) => {
            out.push_str(&format!("r{} = StringConstLen({})", reg.0, idx));
        }
        Instruction::Call(dst, func_ref, args) => {
            out.push_str(&format!("r{} = Call(", dst.0));
            match func_ref {
                FunctionRef::Named(sym) => {
                    out.push_str(&format!("Named(\"{}\")", interner.resolve(*sym)))
                }
                FunctionRef::Extern(sym) => {
                    out.push_str(&format!("Extern(\"{}\")", interner.resolve(*sym)))
                }
                FunctionRef::Builtin(sym) => {
                    out.push_str(&format!("Builtin(\"{}\")", interner.resolve(*sym)))
                }
            }
            out.push_str(", [");
            for (i, r) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", r.0));
            }
            out.push_str("])");
        }
        Instruction::Copy(dst, src) => {
            out.push_str(&format!("r{} = Copy(r{})", dst.0, src.0));
        }
        Instruction::Nop => {
            out.push_str("Nop");
        }
        // Arithmetic
        Instruction::Add(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Add(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Sub(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Sub(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Mul(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Mul(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Div(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Div(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Mod(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Mod(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Shl(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Shl(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Shr(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Shr(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Neg(dst, src) => {
            out.push_str(&format!("r{} = Neg(r{})", dst.0, src.0));
        }
        // Comparison
        Instruction::Eq(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Eq(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Ne(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Ne(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Lt(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Lt(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Le(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Le(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Gt(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Gt(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Ge(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Ge(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        // Logical
        Instruction::Not(dst, src) => {
            out.push_str(&format!("r{} = Not(r{})", dst.0, src.0));
        }
    }
}

fn fmt_terminator(term: &Terminator, out: &mut String) {
    match term {
        Terminator::Return(reg) => out.push_str(&format!("Return(r{})", reg.0)),
        Terminator::Jump(block) => out.push_str(&format!("Jump(b{})", block.0)),
        Terminator::Branch(cond, t, f) => {
            out.push_str(&format!("Branch(r{}, b{}, b{})", cond.0, t.0, f.0))
        }
        Terminator::Unreachable => out.push_str("Unreachable"),
    }
}

