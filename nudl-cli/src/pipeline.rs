use std::path::Path;

use nudl_ast::ast::*;
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_backend_arm64::codegen::{Codegen, CodegenResult};
use nudl_bc::checker::Checker;
use nudl_bc::ir::*;
use nudl_bc::lower::Lowerer;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::source::SourceMap;
use nudl_packer_macho::packer;
use nudl_vm::Vm;

#[derive(Default)]
pub struct DumpOptions {
    pub dump_ast: bool,
    pub dump_ir: bool,
    pub dump_asm: bool,
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

pub fn build(source_path: &Path, output_path: &Path, dump: &DumpOptions) -> CompileResult {
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

    let codegen_result = Codegen::new().generate(&program);

    if dump.dump_asm {
        eprintln!("{}", fmt_asm(&codegen_result));
    }

    match packer::pack(&codegen_result, output_path) {
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
            Literal::Int(s) => out.push_str(&format!("Literal(Int {})", s)),
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

// --- ASM dump ---

fn fmt_asm(codegen: &CodegenResult) -> String {
    let mut out = String::new();

    // Data section
    out.push_str(".data:\n");
    for (i, &(offset, len)) in codegen.string_offsets.iter().enumerate() {
        let bytes = &codegen.data[offset as usize..(offset + len) as usize];
        out.push_str(&format!("  {:04x}: ", offset));
        for (j, b) in bytes.iter().enumerate() {
            if j > 0 && j % 16 == 0 {
                out.push_str(&format!("\n        {:04x}: ", offset + j as u32));
            }
            out.push_str(&format!("{:02x} ", b));
        }
        // Show string value
        let s = String::from_utf8_lossy(bytes);
        out.push_str(&format!(" // str[{}] {:?}", i, s));
        out.push('\n');
    }
    out.push('\n');

    // Text section
    out.push_str(".text:\n");
    for func_sym in &codegen.function_symbols {
        let entry_marker = if func_sym.is_entry { " [entry]" } else { "" };
        out.push_str(&format!(
            "  {} (offset 0x{:x}, {} bytes){}:\n",
            func_sym.name, func_sym.offset, func_sym.size, entry_marker
        ));

        let start = func_sym.offset as usize;
        let end = start + func_sym.size as usize;
        let func_code = &codegen.code[start..end];

        for (i, chunk) in func_code.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let offset = func_sym.offset + (i * 4) as u32;
                out.push_str(&format!(
                    "    {:04x}: {:02x} {:02x} {:02x} {:02x}  {}",
                    offset,
                    chunk[0],
                    chunk[1],
                    chunk[2],
                    chunk[3],
                    disasm_arm64(word)
                ));

                // Annotate relocations at this offset
                for reloc in &codegen.relocations {
                    if reloc.offset == offset {
                        let target_str = match &reloc.target {
                            nudl_backend_arm64::codegen::RelocTarget::DataSection(off) => {
                                let str_idx = codegen
                                    .string_offsets
                                    .iter()
                                    .position(|&(o, _)| o == *off)
                                    .map(|i| format!("str[{}]", i))
                                    .unwrap_or_else(|| format!("data+0x{:x}", off));
                                str_idx
                            }
                            nudl_backend_arm64::codegen::RelocTarget::ExternSymbol(idx) => {
                                format!("extern {}", codegen.extern_symbols[*idx])
                            }
                        };
                        let kind_str = match reloc.kind {
                            nudl_backend_arm64::codegen::RelocKind::Page21 => "PAGE21",
                            nudl_backend_arm64::codegen::RelocKind::PageOff12 => "PAGEOFF12",
                            nudl_backend_arm64::codegen::RelocKind::Branch26 => "BRANCH26",
                        };
                        out.push_str(&format!("  ; {} -> {}", kind_str, target_str));
                    }
                }

                out.push('\n');
            }
        }
    }

    // Relocations summary
    if !codegen.relocations.is_empty() {
        out.push_str("\nrelocations:\n");
        for reloc in &codegen.relocations {
            let kind_str = match reloc.kind {
                nudl_backend_arm64::codegen::RelocKind::Page21 => "PAGE21",
                nudl_backend_arm64::codegen::RelocKind::PageOff12 => "PAGEOFF12",
                nudl_backend_arm64::codegen::RelocKind::Branch26 => "BRANCH26",
            };
            let target_str = match &reloc.target {
                nudl_backend_arm64::codegen::RelocTarget::DataSection(off) => {
                    format!("data+0x{:x}", off)
                }
                nudl_backend_arm64::codegen::RelocTarget::ExternSymbol(idx) => {
                    format!("extern {}", codegen.extern_symbols[*idx])
                }
            };
            out.push_str(&format!(
                "  0x{:04x}: {:10} -> {}\n",
                reloc.offset, kind_str, target_str
            ));
        }
    }

    out
}

/// Basic ARM64 instruction disassembly for common instructions
fn disasm_arm64(word: u32) -> String {
    let rd = word & 0x1f;
    let rn = (word >> 5) & 0x1f;
    let rm = (word >> 16) & 0x1f;

    // STP/LDP pre/post indexed
    if word & 0xffc00000 == 0xa9800000 {
        let rt2 = (word >> 10) & 0x1f;
        let imm7 = ((word >> 15) & 0x7f) as i32;
        let offset = (if imm7 & 0x40 != 0 { imm7 | !0x7f } else { imm7 }) * 8;
        return format!("STP X{}, X{}, [SP, #{}]!", rd, rt2, offset);
    }
    if word & 0xffc00000 == 0xa8c00000 {
        let rt2 = (word >> 10) & 0x1f;
        let imm7 = (word >> 15) & 0x7f;
        return format!("LDP X{}, X{}, [SP], #{}", rd, rt2, imm7 * 8);
    }
    // STP signed offset
    if word & 0xffc00000 == 0xa9000000 {
        let rt2 = (word >> 10) & 0x1f;
        let imm7 = (word >> 15) & 0x7f;
        return format!("STP X{}, X{}, [X{}, #{}]", rd, rt2, rn, imm7 * 8);
    }
    // LDP signed offset
    if word & 0xffc00000 == 0xa9400000 {
        let rt2 = (word >> 10) & 0x1f;
        let imm7 = (word >> 15) & 0x7f;
        return format!("LDP X{}, X{}, [X{}, #{}]", rd, rt2, rn, imm7 * 8);
    }

    // MOV X29, SP (ADD X29, SP, #0)
    if word == 0x910003fd {
        return "MOV X29, SP".into();
    }

    // ADD immediate
    if word & 0xff000000 == 0x91000000 {
        let imm12 = (word >> 10) & 0xfff;
        return format!("ADD X{}, X{}, #{}", rd, rn, imm12);
    }

    // ORR (MOV reg)
    if word & 0xff200000 == 0xaa000000 && rn == 31 {
        return format!("MOV X{}, X{}", rd, rm);
    }

    // ADRP
    if word & 0x9f000000 == 0x90000000 {
        return format!("ADRP X{}, #<page>", rd);
    }

    // BL
    if word & 0xfc000000 == 0x94000000 {
        let imm26 = word & 0x03ffffff;
        let offset = if imm26 & 0x02000000 != 0 {
            ((imm26 | 0xfc000000) as i32) * 4
        } else {
            (imm26 as i32) * 4
        };
        return format!("BL #{:+}", offset);
    }

    // MOVZ
    if word & 0xff800000 == 0xd2800000 {
        let hw = (word >> 21) & 0x3;
        let imm16 = (word >> 5) & 0xffff;
        if hw == 0 {
            return format!("MOVZ X{}, #{}", rd, imm16);
        } else {
            return format!("MOVZ X{}, #{}, LSL #{}", rd, imm16, hw * 16);
        }
    }

    // MOVK
    if word & 0xff800000 == 0xf2800000 {
        let hw = (word >> 21) & 0x3;
        let imm16 = (word >> 5) & 0xffff;
        return format!("MOVK X{}, #{}, LSL #{}", rd, imm16, hw * 16);
    }

    // STR unsigned offset
    if word & 0xffc00000 == 0xf9000000 {
        let imm12 = (word >> 10) & 0xfff;
        return format!("STR X{}, [X{}, #{}]", rd, rn, imm12 * 8);
    }

    // LDR unsigned offset
    if word & 0xffc00000 == 0xf9400000 {
        let imm12 = (word >> 10) & 0xfff;
        return format!("LDR X{}, [X{}, #{}]", rd, rn, imm12 * 8);
    }

    // RET
    if word == 0xd65f03c0 {
        return "RET".into();
    }

    format!("??? 0x{:08x}", word)
}
