mod check_expr;
mod check_items;
mod collect;
mod types;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

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
    /// If this function is generic, holds the template definition
    pub generic_def: Option<GenericFunctionDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    UserDefined,
    Extern,
    Builtin,
}

/// Template for a generic function (stored, not type-checked until monomorphized)
#[derive(Debug, Clone)]
pub struct GenericFunctionDef {
    pub type_params: Vec<TypeParam>,
    pub ast_params: Vec<Param>,
    pub ast_return_type: Option<Spanned<TypeExpr>>,
    pub ast_body: Spanned<Block>,
    pub span: Span,
}

/// Template for a generic struct
#[derive(Debug, Clone)]
pub struct GenericStructDef {
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<StructField>,
    pub span: Span,
}

/// Template for a generic enum
#[derive(Debug, Clone)]
pub struct GenericEnumDef {
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<EnumVariantDef>,
    pub span: Span,
}

/// Template for a generic impl method
#[derive(Debug, Clone)]
pub struct GenericImplMethod {
    pub type_params: Vec<TypeParam>,
    pub method_name: String,
    pub ast_params: Vec<Param>,
    pub ast_return_type: Option<Spanned<TypeExpr>>,
    pub ast_body: Spanned<Block>,
    pub span: Span,
    pub is_pub: bool,
}

pub struct CheckedModule {
    pub functions: HashMap<String, FunctionSig>,
    pub structs: HashMap<String, TypeId>,
    pub enums: HashMap<String, TypeId>,
    pub interfaces: HashMap<String, TypeId>,
    pub types: TypeInterner,
    /// Bodies for monomorphized functions: mangled_name -> (params, body)
    pub mono_fn_bodies: HashMap<String, (Vec<Param>, Spanned<Block>)>,
    /// Generic call site -> mangled function name
    pub call_resolutions: HashMap<Span, String>,
    /// Generic struct literal -> mangled struct name
    pub struct_resolutions: HashMap<Span, String>,
    /// Generic enum constructor -> mangled enum name
    pub enum_resolutions: HashMap<Span, String>,
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
    pub(super) require_main: bool,
    pub(super) current_return_type: Option<TypeId>,
    /// Hint for inferring closure parameter types from the expected function type.
    pub(super) closure_type_hint: Option<TypeId>,

    // --- Generics support ---
    /// Generic struct templates (not yet monomorphized)
    pub(super) generic_structs: HashMap<String, GenericStructDef>,
    /// Generic enum templates (not yet monomorphized)
    pub(super) generic_enums: HashMap<String, GenericEnumDef>,
    /// Generic impl methods keyed by base type name
    pub(super) generic_impl_methods: HashMap<String, Vec<GenericImplMethod>>,
    /// Already-instantiated mangled names (prevents duplicate monomorphization)
    pub(super) mono_cache: HashSet<String>,
    /// Monomorphized function bodies pending type-checking (name, params, body, subst map)
    pub(super) pending_mono_checks:
        Vec<(String, Vec<Param>, Spanned<Block>, HashMap<String, TypeId>)>,
    /// Type parameter scope: "T" -> TypeVar TypeId (during bound checking)
    pub(super) type_param_scope: HashMap<String, TypeId>,
    /// Generic call site -> mangled function name
    pub(super) call_resolutions: HashMap<Span, String>,
    /// Generic struct literal -> mangled struct name
    pub(super) struct_resolutions: HashMap<Span, String>,
    /// Generic enum constructor -> mangled enum name
    pub(super) enum_resolutions: HashMap<Span, String>,
    /// Bodies for monomorphized functions
    pub(super) mono_fn_bodies: HashMap<String, (Vec<Param>, Spanned<Block>)>,
    /// Reverse mapping: mangled type name -> (base_name, concrete type args)
    /// Used during type inference to extract type args from monomorphized generic types.
    pub(super) mono_type_args: HashMap<String, (String, Vec<TypeId>)>,
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
            require_main: true,
            current_return_type: None,
            closure_type_hint: None,
            generic_structs: HashMap::new(),
            generic_enums: HashMap::new(),
            generic_impl_methods: HashMap::new(),
            mono_cache: HashSet::new(),
            pending_mono_checks: Vec::new(),
            type_param_scope: HashMap::new(),
            call_resolutions: HashMap::new(),
            struct_resolutions: HashMap::new(),
            enum_resolutions: HashMap::new(),
            mono_fn_bodies: HashMap::new(),
            mono_type_args: HashMap::new(),
        }
    }

    pub fn require_main(mut self, require: bool) -> Self {
        self.require_main = require;
        self
    }

    pub fn check(mut self, module: &Module) -> (CheckedModule, DiagnosticBag) {
        self.register_builtins();

        // Pass 1: Collect all declarations
        for item in &module.items {
            self.collect_item(item);
        }

        if self.require_main && !self.found_main {
            self.diagnostics.add(&CheckerDiagnostic::NoMainFunction {
                span: Span::dummy(),
            });
        }

        // Pass 2: Check function bodies
        for item in &module.items {
            self.check_item(item);
        }

        // Pass 3: Process monomorphized function bodies until fixpoint
        let mut iterations = 0;
        while !self.pending_mono_checks.is_empty() {
            iterations += 1;
            if iterations > 100 {
                break; // prevent infinite recursion
            }
            let pending = std::mem::take(&mut self.pending_mono_checks);
            for (name, params, body, subst) in pending {
                // Set type_param_scope so type annotations like K[] resolve correctly
                let old_scope = std::mem::replace(&mut self.type_param_scope, subst);
                self.check_fn_body(&name, &params, &body);
                self.type_param_scope = old_scope;
            }
        }

        let checked = CheckedModule {
            functions: self.functions,
            structs: self.structs,
            enums: self.enums,
            interfaces: self.interfaces,
            types: self.types,
            mono_fn_bodies: self.mono_fn_bodies,
            call_resolutions: self.call_resolutions,
            struct_resolutions: self.struct_resolutions,
            enum_resolutions: self.enum_resolutions,
        };
        (checked, self.diagnostics)
    }
}
