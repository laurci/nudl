use std::collections::HashMap;

use nudl_ast::ast::*;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::{Span, Spanned};
use nudl_core::types::{TypeId, TypeInterner, TypeKind};

use crate::checker_diagnostic::CheckerDiagnostic;

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<(String, TypeId)>,
    pub return_type: TypeId,
    pub kind: FunctionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    UserDefined,
    Extern,
    Builtin,
}

pub struct CheckedModule {
    pub functions: HashMap<String, FunctionSig>,
    pub types: TypeInterner,
}

pub struct Checker {
    diagnostics: DiagnosticBag,
    types: TypeInterner,
    functions: HashMap<String, FunctionSig>,
    found_main: bool,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            types: TypeInterner::new(),
            functions: HashMap::new(),
            found_main: false,
        }
    }

    pub fn check(mut self, module: &Module) -> (CheckedModule, DiagnosticBag) {
        self.register_builtins();

        // Pass 1: Collect all declarations
        for item in &module.items {
            self.collect_item(item);
        }

        if !self.found_main {
            self.diagnostics.add(&CheckerDiagnostic::NoMainFunction {
                span: Span::dummy(),
            });
        }

        // Pass 2: Check function bodies
        for item in &module.items {
            self.check_item(item);
        }

        let checked = CheckedModule {
            functions: self.functions,
            types: self.types,
        };
        (checked, self.diagnostics)
    }

    fn register_builtins(&mut self) {
        let string_ty = self.types.string();
        let raw_ptr_ty = self.types.raw_ptr();
        let u64_ty = self.types.u64();

        self.functions.insert(
            "__str_ptr".into(),
            FunctionSig {
                name: "__str_ptr".into(),
                params: vec![("s".into(), string_ty)],
                return_type: raw_ptr_ty,
                kind: FunctionKind::Builtin,
            },
        );

        self.functions.insert(
            "__str_len".into(),
            FunctionSig {
                name: "__str_len".into(),
                params: vec![("s".into(), string_ty)],
                return_type: u64_ty,
                kind: FunctionKind::Builtin,
            },
        );
    }

    fn resolve_type(&mut self, ty: &Spanned<TypeExpr>) -> TypeId {
        match &ty.node {
            TypeExpr::Unit => self.types.unit(),
            TypeExpr::Named(name) => match name.as_str() {
                "i32" => self.types.i32(),
                "i64" => self.types.i64(),
                "u64" => self.types.u64(),
                "bool" => self.types.bool(),
                "string" => self.types.string(),
                "RawPtr" => self.types.raw_ptr(),
                _ => {
                    self.diagnostics.add(&CheckerDiagnostic::UnknownType {
                        span: ty.span,
                        name: name.clone(),
                    });
                    self.types.error()
                }
            },
        }
    }

    fn type_name(&self, ty: TypeId) -> String {
        match self.types.resolve(ty) {
            TypeKind::Primitive(p) => format!("{:?}", p).to_lowercase(),
            TypeKind::String => "string".into(),
            TypeKind::RawPtr => "RawPtr".into(),
            TypeKind::Function { .. } => "fn(...)".into(),
            TypeKind::Error => "<error>".into(),
        }
    }

    // --- Pass 1: Collect declarations ---

    fn collect_item(&mut self, item: &SpannedItem) {
        match &item.node {
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
                if name == "main" {
                    self.found_main = true;
                    if !params.is_empty() || return_type.is_some() {
                        self.diagnostics
                            .add(&CheckerDiagnostic::InvalidMainSignature { span: item.span });
                    }
                }

                if self.functions.contains_key(name) {
                    self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                        span: item.span,
                        name: name.clone(),
                    });
                    return;
                }

                let resolved_params: Vec<(String, TypeId)> = params
                    .iter()
                    .map(|p| (p.name.clone(), self.resolve_type(&p.ty)))
                    .collect();

                let ret_ty = return_type
                    .as_ref()
                    .map(|t| self.resolve_type(t))
                    .unwrap_or_else(|| self.types.unit());

                self.functions.insert(
                    name.clone(),
                    FunctionSig {
                        name: name.clone(),
                        params: resolved_params,
                        return_type: ret_ty,
                        kind: FunctionKind::UserDefined,
                    },
                );
            }
            Item::ExternBlock { items, .. } => {
                for extern_fn in items {
                    let decl = &extern_fn.node;

                    if self.functions.contains_key(&decl.name) {
                        self.diagnostics.add(&CheckerDiagnostic::DuplicateFunction {
                            span: extern_fn.span,
                            name: decl.name.clone(),
                        });
                        continue;
                    }

                    let resolved_params: Vec<(String, TypeId)> = decl
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), self.resolve_type(&p.ty)))
                        .collect();

                    let ret_ty = decl
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type(t))
                        .unwrap_or_else(|| self.types.unit());

                    self.functions.insert(
                        decl.name.clone(),
                        FunctionSig {
                            name: decl.name.clone(),
                            params: resolved_params,
                            return_type: ret_ty,
                            kind: FunctionKind::Extern,
                        },
                    );
                }
            }
        }
    }

    // --- Pass 2: Check bodies ---

    fn check_item(&mut self, item: &SpannedItem) {
        if let Item::FnDef { name, body, .. } = &item.node {
            let mut locals: HashMap<String, TypeId> = HashMap::new();

            // Register params as locals
            if let Some(sig) = self.functions.get(name).cloned() {
                for (pname, pty) in &sig.params {
                    locals.insert(pname.clone(), *pty);
                }
            }

            self.check_block(&body.node, &mut locals);
        }
    }

    fn check_block(&mut self, block: &Block, locals: &mut HashMap<String, TypeId>) {
        for stmt in &block.stmts {
            self.check_stmt(stmt, locals);
        }
    }

    fn check_stmt(&mut self, stmt: &SpannedStmt, locals: &mut HashMap<String, TypeId>) {
        match &stmt.node {
            Stmt::Expr(expr) => {
                self.check_expr(expr, locals);
            }
            Stmt::Let { name, value, .. } => {
                let ty = self.check_expr(value, locals);
                locals.insert(name.clone(), ty);
            }
            Stmt::Item(item) => self.collect_item(item),
        }
    }

    fn check_expr(&mut self, expr: &SpannedExpr, locals: &mut HashMap<String, TypeId>) -> TypeId {
        match &expr.node {
            Expr::Literal(Literal::String(_)) => self.types.string(),
            Expr::Literal(Literal::Int(_)) => self.types.i32(),
            Expr::Literal(Literal::Float(_)) => self.types.i32(),
            Expr::Literal(Literal::Bool(_)) => self.types.bool(),
            Expr::Literal(Literal::Char(_)) => self.types.i32(),

            Expr::Ident(name) => {
                if let Some(&ty) = locals.get(name) {
                    ty
                } else {
                    self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                        span: expr.span,
                        name: name.clone(),
                    });
                    self.types.error()
                }
            }

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    let sig = self.functions.get(name).cloned();
                    if let Some(sig) = sig {
                        // Check argument count
                        if args.len() != sig.params.len() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ArgumentCountMismatch {
                                    span: expr.span,
                                    expected: sig.params.len().to_string(),
                                    found: args.len().to_string(),
                                });
                        } else {
                            // Check argument types
                            for (i, arg) in args.iter().enumerate() {
                                let arg_ty = self.check_expr(&arg.value, locals);
                                let param_ty = sig.params[i].1;
                                if arg_ty != param_ty
                                    && arg_ty != self.types.error()
                                    && param_ty != self.types.error()
                                {
                                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                        span: arg.value.span,
                                        expected: self.type_name(param_ty),
                                        found: self.type_name(arg_ty),
                                    });
                                }
                            }
                        }
                        sig.return_type
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedFunction {
                            span: callee.span,
                            name: name.clone(),
                        });
                        for arg in args {
                            self.check_expr(&arg.value, locals);
                        }
                        self.types.error()
                    }
                } else {
                    self.check_expr(callee, locals);
                    for arg in args {
                        self.check_expr(&arg.value, locals);
                    }
                    self.types.error()
                }
            }

            Expr::Block(block) => {
                self.check_block(block, locals);
                self.types.unit()
            }

            Expr::Return(Some(inner)) => {
                self.check_expr(inner, locals);
                self.types.unit()
            }
            Expr::Return(None) => self.types.unit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_core::span::FileId;

    fn check_source(source: &str) -> (CheckedModule, DiagnosticBag) {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        Checker::new().check(&module)
    }

    #[test]
    fn extern_functions_registered() {
        let (checked, diags) = check_source(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn main() {}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
        assert!(checked.functions.contains_key("write"));
        let sig = &checked.functions["write"];
        assert_eq!(sig.kind, FunctionKind::Extern);
        assert_eq!(sig.params.len(), 3);
    }

    #[test]
    fn undefined_function_error() {
        let (_, diags) = check_source(
            r#"
fn main() {
    foo();
}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("undefined function 'foo'"))
        );
    }

    #[test]
    fn argument_count_mismatch() {
        let (_, diags) = check_source(
            r#"
fn greet(s: string) {}
fn main() {
    greet("a", "b");
}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("argument"))
        );
    }

    #[test]
    fn type_mismatch() {
        let (_, diags) = check_source(
            r#"
fn greet(s: string) {}
fn main() {
    greet(42);
}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("type mismatch"))
        );
    }

    #[test]
    fn builtins_recognized() {
        let (checked, diags) = check_source(
            r#"
fn main() {
    __str_ptr("hello");
    __str_len("hello");
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
        assert!(checked.functions.contains_key("__str_ptr"));
        assert!(checked.functions.contains_key("__str_len"));
        assert_eq!(checked.functions["__str_ptr"].kind, FunctionKind::Builtin);
    }

    #[test]
    fn main_validation_preserved() {
        let (_, diags) = check_source("fn foo() {}");
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("no 'main' function"))
        );
    }

    #[test]
    fn user_defined_function_registered() {
        let (checked, diags) = check_source(
            r#"
fn print(s: string) {}
fn main() {
    print("hello");
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
        assert!(checked.functions.contains_key("print"));
        let sig = &checked.functions["print"];
        assert_eq!(sig.kind, FunctionKind::UserDefined);
        assert_eq!(sig.params.len(), 1);
    }

    #[test]
    fn duplicate_function_error() {
        let (_, diags) = check_source(
            r#"
fn foo() {}
fn foo() {}
fn main() {}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("duplicate function"))
        );
    }

    #[test]
    fn unknown_type_error() {
        let (_, diags) = check_source(
            r#"
fn foo(x: Blah) {}
fn main() {}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("unknown type"))
        );
    }

    #[test]
    fn target_program_passes() {
        let (checked, diags) = check_source(
            r#"
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
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
        assert_eq!(checked.functions["write"].kind, FunctionKind::Extern);
        assert_eq!(checked.functions["print"].kind, FunctionKind::UserDefined);
        assert_eq!(checked.functions["println"].kind, FunctionKind::UserDefined);
    }
}
