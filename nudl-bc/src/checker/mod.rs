mod check_expr;
mod check_items;
mod collect;
mod types;

#[cfg(test)]
mod tests;

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
    /// Number of required (non-default) parameters
    pub required_params: usize,
    /// Whether each parameter has a default value (indices correspond to params)
    pub has_default: Vec<bool>,
    /// Whether the first parameter is `self` (method)
    pub is_method: bool,
    /// Whether the first parameter is `mut self`
    pub is_mut_method: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    UserDefined,
    Extern,
    Builtin,
}

pub struct CheckedModule {
    pub functions: HashMap<String, FunctionSig>,
    pub structs: HashMap<String, TypeId>,
    pub enums: HashMap<String, TypeId>,
    pub interfaces: HashMap<String, TypeId>,
    pub types: TypeInterner,
}

#[derive(Debug, Clone)]
pub(super) struct LocalInfo {
    pub(super) ty: TypeId,
    pub(super) is_mut: bool,
}

pub struct Checker {
    pub(super) diagnostics: DiagnosticBag,
    pub(super) types: TypeInterner,
    pub(super) functions: HashMap<String, FunctionSig>,
    pub(super) structs: HashMap<String, TypeId>,
    pub(super) enums: HashMap<String, TypeId>,
    pub(super) interfaces: HashMap<String, TypeId>,
    /// Map from interface name → set of type names that implement it
    pub(super) interface_impls: HashMap<String, Vec<String>>,
    /// Type aliases: name → resolved TypeId
    pub(super) type_aliases: HashMap<String, TypeId>,
    pub(super) found_main: bool,
    pub(super) current_return_type: Option<TypeId>,
    /// Hint for inferring closure parameter types from the expected function type.
    pub(super) closure_type_hint: Option<TypeId>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            types: TypeInterner::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            interfaces: HashMap::new(),
            interface_impls: HashMap::new(),
            type_aliases: HashMap::new(),
            found_main: false,
            current_return_type: None,
            closure_type_hint: None,
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
            structs: self.structs,
            enums: self.enums,
            interfaces: self.interfaces,
            types: self.types,
        };
        (checked, self.diagnostics)
    }
}
