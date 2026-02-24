use std::collections::HashMap;

use nudl_core::intern::StringInterner;
use nudl_core::types::TypeInterner;

use nudl_ast::ast::*;

use crate::checker::{CheckedModule, FunctionKind, FunctionSig};
use crate::ir::*;

/// Lowers AST to SSA bytecode. Consumes CheckedModule for function signatures.
pub struct Lowerer {
    interner: StringInterner,
    _types: TypeInterner,
    function_sigs: HashMap<String, FunctionSig>,
    functions: Vec<Function>,
    string_constants: Vec<String>,
    next_function_id: u32,
}

impl Lowerer {
    pub fn new(checked: CheckedModule) -> Self {
        Self {
            interner: StringInterner::new(),
            _types: checked.types,
            function_sigs: checked.functions,
            functions: Vec::new(),
            string_constants: Vec::new(),
            next_function_id: 0,
        }
    }

    pub fn lower(mut self, module: &Module) -> Program {
        let mut entry_function = None;

        // Pass 1: Register extern functions
        for item in &module.items {
            if let Item::ExternBlock { items, .. } = &item.node {
                for extern_fn in items {
                    let decl = &extern_fn.node;
                    let func = self.lower_extern_function(&decl.name);
                    self.functions.push(func);
                }
            }
        }

        // Pass 2: Lower user-defined functions
        for item in &module.items {
            if let Item::FnDef { name, params, body, .. } = &item.node {
                let func = self.lower_function(name, params, body);
                if name == "main" {
                    entry_function = Some(func.id);
                }
                self.functions.push(func);
            }
        }

        Program {
            functions: self.functions,
            string_constants: self.string_constants,
            entry_function,
            extern_libs: vec!["System".into()],
            interner: self.interner,
        }
    }

    fn lower_extern_function(&mut self, name: &str) -> Function {
        let id = FunctionId(self.next_function_id);
        self.next_function_id += 1;
        let name_sym = self.interner.intern(name);

        let sig = self.function_sigs.get(name).unwrap().clone();

        let params: Vec<(nudl_core::intern::Symbol, nudl_core::types::TypeId)> = sig.params.iter()
            .map(|(pname, pty)| (self.interner.intern(pname), *pty))
            .collect();

        Function {
            id,
            name: name_sym,
            params,
            return_type: sig.return_type,
            blocks: vec![],
            register_count: 0,
            is_extern: true,
            extern_symbol: Some(name.to_string()),
        }
    }

    fn lower_function(&mut self, name: &str, params: &[Param], body: &nudl_core::span::Spanned<Block>) -> Function {
        let id = FunctionId(self.next_function_id);
        self.next_function_id += 1;
        let name_sym = self.interner.intern(name);

        let sig = self.function_sigs.get(name).unwrap().clone();

        let ir_params: Vec<(nudl_core::intern::Symbol, nudl_core::types::TypeId)> = sig.params.iter()
            .map(|(pname, pty)| (self.interner.intern(pname), *pty))
            .collect();

        // Build locals map from params: param[i].name → Register(i)
        let mut locals: HashMap<String, Register> = HashMap::new();
        let mut next_register = 0u32;
        for param in params {
            locals.insert(param.name.clone(), Register(next_register));
            next_register += 1;
        }

        let mut ctx = FunctionLowerCtx {
            instructions: Vec::new(),
            next_register,
            locals,
            string_constants: &mut self.string_constants,
            interner: &mut self.interner,
            function_sigs: &self.function_sigs,
        };

        ctx.lower_block(&body.node);

        // Ensure function ends with a return
        let ret_reg = ctx.alloc_register();
        ctx.instructions.push(Instruction::ConstUnit(ret_reg));

        let register_count = ctx.next_register;
        let instructions = ctx.instructions;

        let block = BasicBlock {
            id: BlockId(0),
            instructions,
            terminator: Terminator::Return(ret_reg),
        };

        Function {
            id,
            name: name_sym,
            params: ir_params,
            return_type: sig.return_type,
            blocks: vec![block],
            register_count,
            is_extern: false,
            extern_symbol: None,
        }
    }
}

struct FunctionLowerCtx<'a> {
    instructions: Vec<Instruction>,
    next_register: u32,
    locals: HashMap<String, Register>,
    string_constants: &'a mut Vec<String>,
    interner: &'a mut StringInterner,
    function_sigs: &'a HashMap<String, FunctionSig>,
}

impl<'a> FunctionLowerCtx<'a> {
    fn alloc_register(&mut self) -> Register {
        let r = Register(self.next_register);
        self.next_register += 1;
        r
    }

    fn lower_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
    }

    fn lower_stmt(&mut self, stmt: &nudl_core::span::Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Expr(expr) => { self.lower_expr(expr); }
            Stmt::Let { name, value, .. } => {
                let reg = self.lower_expr(value);
                self.locals.insert(name.clone(), reg);
            }
            Stmt::Item(_) => {} // nested items not supported yet
        }
    }

    fn lower_expr(&mut self, expr: &nudl_core::span::Spanned<Expr>) -> Register {
        match &expr.node {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(name).cloned() {
                        return match sig.kind {
                            FunctionKind::Builtin => self.lower_builtin_call(name, args),
                            FunctionKind::Extern => self.lower_generic_call(name, args, true),
                            FunctionKind::UserDefined => self.lower_generic_call(name, args, false),
                        };
                    }
                }
                // Fallback: emit unit
                let unit_reg = self.alloc_register();
                self.instructions.push(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Literal(Literal::String(s)) => {
                // Deduplicate string constants
                let idx = if let Some(pos) = self.string_constants.iter().position(|c| c == s) {
                    pos as u32
                } else {
                    let idx = self.string_constants.len() as u32;
                    self.string_constants.push(s.clone());
                    idx
                };
                let reg = self.alloc_register();
                self.instructions.push(Instruction::Const(reg, ConstValue::StringLiteral(idx)));
                reg
            }

            Expr::Literal(Literal::Int(s)) => {
                let val: i32 = s.parse().unwrap_or(0);
                let reg = self.alloc_register();
                self.instructions.push(Instruction::Const(reg, ConstValue::I32(val)));
                reg
            }

            Expr::Literal(Literal::Bool(b)) => {
                let reg = self.alloc_register();
                self.instructions.push(Instruction::Const(reg, ConstValue::Bool(*b)));
                reg
            }

            Expr::Ident(name) => {
                if let Some(&reg) = self.locals.get(name) {
                    reg
                } else {
                    // Should have been caught by checker
                    let reg = self.alloc_register();
                    self.instructions.push(Instruction::ConstUnit(reg));
                    reg
                }
            }

            Expr::Return(Some(inner)) => {
                self.lower_expr(inner)
            }

            _ => {
                let reg = self.alloc_register();
                self.instructions.push(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    fn lower_builtin_call(&mut self, name: &str, args: &[CallArg]) -> Register {
        match name {
            "__str_ptr" => {
                let arg_reg = self.lower_expr(&args[0].value);
                let dst = self.alloc_register();
                self.instructions.push(Instruction::StringPtr(dst, arg_reg));
                dst
            }
            "__str_len" => {
                let arg_reg = self.lower_expr(&args[0].value);
                let dst = self.alloc_register();
                self.instructions.push(Instruction::StringLen(dst, arg_reg));
                dst
            }
            _ => {
                let reg = self.alloc_register();
                self.instructions.push(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    fn lower_generic_call(&mut self, name: &str, args: &[CallArg], is_extern: bool) -> Register {
        // Lower all arguments
        let arg_regs: Vec<Register> = args.iter()
            .map(|arg| self.lower_expr(&arg.value))
            .collect();

        let sym = self.interner.intern(name);

        let func_ref = if is_extern {
            FunctionRef::Extern(sym)
        } else {
            FunctionRef::Named(sym)
        };

        let dst = self.alloc_register();
        self.instructions.push(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_core::span::FileId;
    use crate::checker::Checker;

    fn lower_source(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        Lowerer::new(checked).lower(&module)
    }

    #[test]
    fn lower_target_program() {
        let program = lower_source(r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}

fn println(s: string) {
    print(s);
    print("\n");
}

fn main() {
    println("Hello, world!");
}
"#);

        // 4 functions: write (extern), print, println, main
        assert_eq!(program.functions.len(), 4, "expected 4 functions, got {}", program.functions.len());

        // write should be extern
        let write_fn = &program.functions[0];
        assert!(write_fn.is_extern);
        assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));

        // String constants should include "Hello, world!" and "\n"
        assert!(program.string_constants.contains(&"Hello, world!".to_string()),
            "missing 'Hello, world!' in {:?}", program.string_constants);
        assert!(program.string_constants.contains(&"\n".to_string()),
            "missing '\\n' in {:?}", program.string_constants);

        // Entry function should be main
        assert!(program.entry_function.is_some());

        // print function should have StringPtr and StringLen instructions
        let print_fn = &program.functions[1];
        assert!(!print_fn.is_extern);
        assert_eq!(print_fn.params.len(), 1);
        let block = &print_fn.blocks[0];
        let has_str_ptr = block.instructions.iter().any(|i| matches!(i, Instruction::StringPtr(_, _)));
        let has_str_len = block.instructions.iter().any(|i| matches!(i, Instruction::StringLen(_, _)));
        assert!(has_str_ptr, "print should have StringPtr instruction");
        assert!(has_str_len, "print should have StringLen instruction");
    }

    #[test]
    fn lower_has_return() {
        let program = lower_source(r#"
fn main() {
    __str_ptr("hi");
}
"#);
        let main_func = program.functions.iter().find(|f| !f.is_extern).unwrap();
        let block = &main_func.blocks[0];
        assert!(matches!(block.terminator, Terminator::Return(_)));
    }

    #[test]
    fn extern_function_lowered() {
        let program = lower_source(r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn main() {}
"#);
        let write_fn = program.functions.iter().find(|f| f.is_extern).unwrap();
        assert!(write_fn.blocks.is_empty());
        assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));
    }

    #[test]
    fn params_assigned_to_registers() {
        let program = lower_source(r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#);
        let greet_fn = &program.functions[0];
        assert_eq!(greet_fn.params.len(), 1);
        // param register 0 is used (next_register starts at 1 for the body)
    }

    #[test]
    fn string_dedup() {
        let program = lower_source(r#"
fn main() {
    __str_ptr("same");
    __str_ptr("same");
}
"#);
        // "same" should appear only once
        assert_eq!(program.string_constants.iter().filter(|s| *s == "same").count(), 1);
    }
}
