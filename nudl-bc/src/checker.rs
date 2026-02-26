use std::collections::HashMap;

use nudl_ast::ast::*;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::{Span, Spanned};
use nudl_core::types::{PrimitiveType, TypeId, TypeInterner, TypeKind};

use crate::checker_diagnostic::CheckerDiagnostic;
use crate::scoped_locals::ScopedLocals;

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

#[derive(Debug, Clone)]
struct LocalInfo {
    ty: TypeId,
    is_mut: bool,
}

pub struct Checker {
    diagnostics: DiagnosticBag,
    types: TypeInterner,
    functions: HashMap<String, FunctionSig>,
    found_main: bool,
    /// Return type of the current function being checked
    current_return_type: Option<TypeId>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            types: TypeInterner::new(),
            functions: HashMap::new(),
            found_main: false,
            current_return_type: None,
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
                "i8" => self.types.i8(),
                "i16" => self.types.i16(),
                "i32" => self.types.i32(),
                "i64" => self.types.i64(),
                "u8" => self.types.u8(),
                "u16" => self.types.u16(),
                "u32" => self.types.u32(),
                "u64" => self.types.u64(),
                "f32" => self.types.f32(),
                "f64" => self.types.f64(),
                "bool" => self.types.bool(),
                "char" => self.types.char_type(),
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
            TypeKind::Primitive(p) => match p {
                PrimitiveType::Char => "char".into(),
                p => format!("{:?}", p).to_lowercase(),
            },
            TypeKind::String => "string".into(),
            TypeKind::RawPtr => "RawPtr".into(),
            TypeKind::Function { .. } => "fn(...)".into(),
            TypeKind::Error => "<error>".into(),
        }
    }

    fn is_numeric(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_numeric()
        )
    }

    fn is_integer_type(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_integer()
        )
    }

    fn is_unsuffixed_int_literal(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(Literal::Int(_, None)))
            || matches!(expr, Expr::Unary { op: UnaryOp::Neg, operand } if matches!(&operand.node, Expr::Literal(Literal::Int(_, None))))
    }

    fn is_signed_or_float(&self, ty: TypeId) -> bool {
        matches!(
            self.types.resolve(ty),
            TypeKind::Primitive(p) if p.is_signed()
        )
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
            let mut locals = ScopedLocals::<LocalInfo>::new();

            // Register params as locals (immutable by default)
            let sig = self.functions.get(name).cloned();
            let ret_ty = if let Some(ref sig) = sig {
                for (pname, pty) in &sig.params {
                    locals.insert(
                        pname.clone(),
                        LocalInfo {
                            ty: *pty,
                            is_mut: false,
                        },
                    );
                }
                sig.return_type
            } else {
                self.types.unit()
            };

            self.current_return_type = Some(ret_ty);
            let body_ty = self.check_block(&body.node, &mut locals);
            self.current_return_type = None;

            // Check return type
            if body_ty != ret_ty
                && body_ty != self.types.error()
                && ret_ty != self.types.error()
            {
                self.diagnostics
                    .add(&CheckerDiagnostic::ReturnTypeMismatch {
                        span: body.span,
                        expected: self.type_name(ret_ty),
                        found: self.type_name(body_ty),
                    });
            }
        }
    }

    fn check_block(
        &mut self,
        block: &Block,
        locals: &mut ScopedLocals<LocalInfo>,
    ) -> TypeId {
        locals.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt, locals);
        }
        let result = if let Some(tail) = &block.tail_expr {
            self.check_expr(tail, locals)
        } else {
            self.types.unit()
        };
        locals.pop_scope();
        result
    }

    fn check_stmt(&mut self, stmt: &SpannedStmt, locals: &mut ScopedLocals<LocalInfo>) {
        match &stmt.node {
            Stmt::Expr(expr) => {
                self.check_expr(expr, locals);
            }
            Stmt::Let {
                name,
                ty,
                value,
                is_mut,
            } => {
                let val_ty = self.check_expr(value, locals);

                // If explicit type annotation, check it matches
                if let Some(type_expr) = ty {
                    let declared_ty = self.resolve_type(type_expr);
                    // Allow unsuffixed integer literals to coerce to any integer type
                    let is_coercible = self.is_unsuffixed_int_literal(&value.node)
                        && self.is_integer_type(declared_ty);
                    if val_ty != declared_ty
                        && val_ty != self.types.error()
                        && declared_ty != self.types.error()
                        && !is_coercible
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: value.span,
                            expected: self.type_name(declared_ty),
                            found: self.type_name(val_ty),
                        });
                    }
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: declared_ty,
                            is_mut: *is_mut,
                        },
                    );
                } else {
                    locals.insert(
                        name.clone(),
                        LocalInfo {
                            ty: val_ty,
                            is_mut: *is_mut,
                        },
                    );
                }
            }
            Stmt::Item(item) => self.collect_item(item),
        }
    }

    fn check_expr(
        &mut self,
        expr: &SpannedExpr,
        locals: &mut ScopedLocals<LocalInfo>,
    ) -> TypeId {
        match &expr.node {
            Expr::Literal(Literal::String(_)) => self.types.string(),
            Expr::Literal(Literal::TemplateString { exprs, .. }) => {
                for e in exprs {
                    self.check_expr(e, locals);
                }
                self.types.string()
            }
            Expr::Literal(Literal::Int(_, Some(suffix))) => match suffix {
                IntSuffix::I8 => self.types.i8(),
                IntSuffix::I16 => self.types.i16(),
                IntSuffix::I32 => self.types.i32(),
                IntSuffix::I64 => self.types.i64(),
                IntSuffix::U8 => self.types.u8(),
                IntSuffix::U16 => self.types.u16(),
                IntSuffix::U32 => self.types.u32(),
                IntSuffix::U64 => self.types.u64(),
            },
            Expr::Literal(Literal::Int(_, None)) => self.types.i32(),
            Expr::Literal(Literal::Float(_)) => self.types.f64(),
            Expr::Literal(Literal::Bool(_)) => self.types.bool(),
            Expr::Literal(Literal::Char(_)) => self.types.char_type(),

            Expr::Ident(name) => {
                if let Some(info) = locals.get(name) {
                    info.ty
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

            Expr::Block(block) => self.check_block(block, locals),

            Expr::Return(Some(inner)) => {
                let val_ty = self.check_expr(inner, locals);
                if let Some(ret_ty) = self.current_return_type {
                    if val_ty != ret_ty
                        && val_ty != self.types.error()
                        && ret_ty != self.types.error()
                    {
                        self.diagnostics
                            .add(&CheckerDiagnostic::ReturnTypeMismatch {
                                span: inner.span,
                                expected: self.type_name(ret_ty),
                                found: self.type_name(val_ty),
                            });
                    }
                }
                self.types.unit()
            }
            Expr::Return(None) => self.types.unit(),

            Expr::Binary { op, left, right } => {
                let left_ty = self.check_expr(left, locals);
                let right_ty = self.check_expr(right, locals);

                if left_ty == self.types.error() || right_ty == self.types.error() {
                    return self.types.error();
                }

                // Both sides must be same type
                if left_ty != right_ty {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: right.span,
                        expected: self.type_name(left_ty),
                        found: self.type_name(right_ty),
                    });
                    return self.types.error();
                }

                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
                    | BinOp::Shl | BinOp::Shr => {
                        if !self.is_numeric(left_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(left_ty),
                                });
                            return self.types.error();
                        }
                        left_ty
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.types.bool()
                    }
                    BinOp::And | BinOp::Or => {
                        if left_ty != self.types.bool() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(left_ty),
                                });
                            return self.types.error();
                        }
                        self.types.bool()
                    }
                }
            }

            Expr::Unary { op, operand } => {
                let operand_ty = self.check_expr(operand, locals);
                if operand_ty == self.types.error() {
                    return self.types.error();
                }

                match op {
                    UnaryOp::Neg => {
                        if !self.is_signed_or_float(operand_ty) {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: "-".into(),
                                    ty: self.type_name(operand_ty),
                                });
                            return self.types.error();
                        }
                        operand_ty
                    }
                    UnaryOp::Not => {
                        if operand_ty != self.types.bool() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: "!".into(),
                                    ty: self.type_name(operand_ty),
                                });
                            return self.types.error();
                        }
                        self.types.bool()
                    }
                }
            }

            Expr::Assign { target, value } => {
                let val_ty = self.check_expr(value, locals);
                if let Expr::Ident(name) = &target.node {
                    if let Some(info) = locals.get(name) {
                        if !info.is_mut {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ImmutableAssignment {
                                    span: target.span,
                                    name: name.clone(),
                                });
                        }
                        let target_ty = info.ty;
                        if val_ty != target_ty
                            && val_ty != self.types.error()
                            && target_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: value.span,
                                expected: self.type_name(target_ty),
                                found: self.type_name(val_ty),
                            });
                        }
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                            span: target.span,
                            name: name.clone(),
                        });
                    }
                }
                self.types.unit()
            }

            Expr::CompoundAssign { op, target, value } => {
                let val_ty = self.check_expr(value, locals);
                if let Expr::Ident(name) = &target.node {
                    if let Some(info) = locals.get(name) {
                        if !info.is_mut {
                            self.diagnostics
                                .add(&CheckerDiagnostic::ImmutableAssignment {
                                    span: target.span,
                                    name: name.clone(),
                                });
                        }
                        let target_ty = info.ty;
                        if !self.is_numeric(target_ty) && target_ty != self.types.error() {
                            self.diagnostics
                                .add(&CheckerDiagnostic::InvalidOperatorType {
                                    span: expr.span,
                                    op: format!("{:?}", op).to_lowercase(),
                                    ty: self.type_name(target_ty),
                                });
                        }
                        if val_ty != target_ty
                            && val_ty != self.types.error()
                            && target_ty != self.types.error()
                        {
                            self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                                span: value.span,
                                expected: self.type_name(target_ty),
                                found: self.type_name(val_ty),
                            });
                        }
                    } else {
                        self.diagnostics.add(&CheckerDiagnostic::UndefinedVariable {
                            span: target.span,
                            name: name.clone(),
                        });
                    }
                }
                self.types.unit()
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(condition, locals);
                if cond_ty != self.types.bool()
                    && cond_ty != self.types.error()
                {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: condition.span,
                        expected: "bool".into(),
                        found: self.type_name(cond_ty),
                    });
                }

                let then_ty = self.check_block(&then_branch.node, locals);

                if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr, locals);
                    if then_ty != else_ty
                        && then_ty != self.types.error()
                        && else_ty != self.types.error()
                    {
                        self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                            span: else_expr.span,
                            expected: self.type_name(then_ty),
                            found: self.type_name(else_ty),
                        });
                    }
                    then_ty
                } else {
                    self.types.unit()
                }
            }

            Expr::While { condition, body } => {
                let cond_ty = self.check_expr(condition, locals);
                if cond_ty != self.types.bool()
                    && cond_ty != self.types.error()
                {
                    self.diagnostics.add(&CheckerDiagnostic::TypeMismatch {
                        span: condition.span,
                        expected: "bool".into(),
                        found: self.type_name(cond_ty),
                    });
                }
                self.check_block(&body.node, locals);
                self.types.unit()
            }

            Expr::Loop { body } => {
                self.check_block(&body.node, locals);
                self.types.unit()
            }

            Expr::Break(_value) => {
                // break-with-value not fully supported yet
                self.types.unit()
            }

            Expr::Continue => self.types.unit(),

            Expr::Grouped(inner) => self.check_expr(inner, locals),
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

    #[test]
    fn immutable_assignment_error() {
        let (_, diags) = check_source(
            r#"
fn main() {
    let x = 10;
    x = 20;
}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("immutable"))
        );
    }

    #[test]
    fn mutable_assignment_ok() {
        let (_, diags) = check_source(
            r#"
fn main() {
    let mut x = 10;
    x = 20;
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
    }

    #[test]
    fn binary_operator_type_check() {
        let (_, diags) = check_source(
            r#"
fn main() {
    let x: i32 = 10;
    let y: i32 = 20;
    let z = x + y;
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
    }

    #[test]
    fn return_type_check() {
        let (_, diags) = check_source(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    add(1, 2);
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
    }

    #[test]
    fn return_type_mismatch() {
        let (_, diags) = check_source(
            r#"
fn foo() -> i32 {
    true
}
fn main() {}
"#,
        );
        assert!(diags.has_errors());
        assert!(
            diags
                .reports()
                .iter()
                .any(|r| r.message.contains("return type"))
        );
    }

    #[test]
    fn if_condition_must_be_bool() {
        let (_, diags) = check_source(
            r#"
fn main() {
    if 42 {}
}
"#,
        );
        assert!(diags.has_errors());
    }

    #[test]
    fn while_with_comparison() {
        let (_, diags) = check_source(
            r#"
fn main() {
    let mut x: i32 = 0;
    while x < 10 {
        x = x + 1;
    }
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
    }

    #[test]
    fn target_program_v2_passes() {
        let (_, diags) = check_source(
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

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let x: i32 = 10;
    let y = 20;
    let sum = add(x, y);

    if sum > 25 {
        println("big");
    } else {
        println("small");
    }

    let mut counter: i32 = 0;
    while counter < 10 {
        counter = counter + 1;
    }
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );
    }

    #[test]
    fn block_scoping_hides_inner_variables() {
        let (_, diags) = check_source(
            r#"
fn main() {
    let n = {
        let z = 42;
        z
    };
    n;
}
"#,
        );
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.reports()
        );

        // z should NOT be accessible outside the block
        let (_, diags) = check_source(
            r#"
fn id(x: i32) -> i32 { x }
fn main() {
    let n = {
        let z = 42;
        z
    };
    id(z);
}
"#,
        );
        assert!(diags.has_errors(), "z should be undefined outside the block");
    }

    #[test]
    fn if_scoping_hides_inner_variables() {
        let (_, diags) = check_source(
            r#"
fn id(x: i32) -> i32 { x }
fn main() {
    if true {
        let y = 10;
    }
    id(y);
}
"#,
        );
        assert!(diags.has_errors(), "y should be undefined outside the if block");
    }

    #[test]
    fn while_scoping_hides_inner_variables() {
        let (_, diags) = check_source(
            r#"
fn id(x: i32) -> i32 { x }
fn main() {
    let mut x: i32 = 0;
    while x < 1 {
        let inner = 5;
        x = x + 1;
    }
    id(inner);
}
"#,
        );
        assert!(diags.has_errors(), "inner should be undefined outside the while block");
    }
}
