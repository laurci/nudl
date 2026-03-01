mod check_expr;
mod check_items;
mod collect;
mod types;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use nudl_ast::ast::*;
use nudl_core::diagnostic::DiagnosticBag;
use nudl_core::span::{FileId, Span, Spanned};
use nudl_core::types::{PrimitiveType, TypeId, TypeInterner, TypeKind};

use crate::checker_diagnostic::CheckerDiagnostic;
use crate::scoped_locals::ScopedLocals;
use crate::symbol_table::{DefinitionInfo, SymbolKind, SymbolTable};

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
    /// Whether this function is public (accessible from other modules)
    pub is_pub: bool,
    /// Which file this function was defined in
    pub source_file_id: FileId,
    /// The span of the function definition (name span)
    pub def_span: Span,
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

/// Template for a generic interface (has type parameters like Iterator<T>)
#[derive(Debug, Clone)]
pub struct GenericInterfaceDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub methods: Vec<InterfaceMethodDef>,
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
    /// Bodies for monomorphized functions: mangled_name -> (params, body, type_param_subst)
    pub mono_fn_bodies: HashMap<String, (Vec<Param>, Spanned<Block>, HashMap<String, TypeId>)>,
    /// Generic call site -> mangled function name (keyed by (current_fn_name, call_span))
    pub call_resolutions: HashMap<(String, Span), String>,
    /// Generic struct literal -> mangled struct name
    pub struct_resolutions: HashMap<Span, String>,
    /// Generic enum constructor -> mangled enum name
    pub enum_resolutions: HashMap<Span, String>,
    /// Map from interface name → set of type names that implement it
    pub interface_impls: HashMap<String, Vec<String>>,
    /// Interface AST method defs for vtable building
    pub interface_method_defs: HashMap<String, Vec<InterfaceMethodDef>>,
    /// Dynamic dispatch call resolutions: span → (interface_name, method_index)
    pub dyn_call_resolutions: HashMap<Span, (String, usize)>,
    /// Symbol table for IDE features (go-to-def, hover, completions)
    pub symbol_table: SymbolTable,
    /// Item definition spans: item_name → definition span (for find-implementations)
    pub item_def_spans: HashMap<String, Span>,
}

#[derive(Debug, Clone)]
pub(super) struct LocalInfo {
    pub(super) ty: TypeId,
    pub(super) is_mut: bool,
    pub(super) def_span: Span,
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

    // --- Visibility ---
    /// Type name → (is_pub, source_file_id)
    pub(super) type_visibility: HashMap<String, (bool, FileId)>,
    /// Struct name → [(field_name, is_pub)]
    pub(super) field_visibility: HashMap<String, Vec<(String, bool)>>,

    // --- Generics support ---
    /// Generic struct templates (not yet monomorphized)
    pub(super) generic_structs: HashMap<String, GenericStructDef>,
    /// Generic enum templates (not yet monomorphized)
    pub(super) generic_enums: HashMap<String, GenericEnumDef>,
    /// Generic interface templates (interfaces with type params, like Iterator<T>)
    pub(super) generic_interfaces: HashMap<String, GenericInterfaceDef>,
    /// Interface AST method definitions (for default method bodies)
    pub(super) interface_method_defs: HashMap<String, Vec<InterfaceMethodDef>>,
    /// Generic impl methods keyed by base type name
    pub(super) generic_impl_methods: HashMap<String, Vec<GenericImplMethod>>,
    /// Already-instantiated mangled names (prevents duplicate monomorphization)
    pub(super) mono_cache: HashSet<String>,
    /// Monomorphized function bodies pending type-checking (name, params, body, subst map)
    pub(super) pending_mono_checks:
        Vec<(String, Vec<Param>, Spanned<Block>, HashMap<String, TypeId>)>,
    /// Type parameter scope: "T" -> TypeVar TypeId (during bound checking)
    pub(super) type_param_scope: HashMap<String, TypeId>,
    /// Name of the function currently being type-checked (for call resolution context)
    pub(super) current_fn_name: String,
    /// Generic call site -> mangled function name (keyed by (current_fn_name, call_span))
    pub(super) call_resolutions: HashMap<(String, Span), String>,
    /// Generic struct literal -> mangled struct name
    pub(super) struct_resolutions: HashMap<Span, String>,
    /// Generic enum constructor -> mangled enum name
    pub(super) enum_resolutions: HashMap<Span, String>,
    /// Bodies for monomorphized functions (with type param substitution maps)
    pub(super) mono_fn_bodies:
        HashMap<String, (Vec<Param>, Spanned<Block>, HashMap<String, TypeId>)>,
    /// Reverse mapping: mangled type name -> (base_name, concrete type args)
    /// Used during type inference to extract type args from monomorphized generic types.
    pub(super) mono_type_args: HashMap<String, (String, Vec<TypeId>)>,
    /// Dynamic dispatch call resolutions: span → (interface_name, method_index)
    pub(super) dyn_call_resolutions: HashMap<Span, (String, usize)>,
    /// Item definition spans: item_name → definition span
    pub(super) item_def_spans: HashMap<String, Span>,
    /// Symbol table for IDE features
    pub(super) symbol_table: SymbolTable,
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
            type_visibility: HashMap::new(),
            field_visibility: HashMap::new(),
            generic_structs: HashMap::new(),
            generic_enums: HashMap::new(),
            generic_interfaces: HashMap::new(),
            interface_method_defs: HashMap::new(),
            generic_impl_methods: HashMap::new(),
            mono_cache: HashSet::new(),
            pending_mono_checks: Vec::new(),
            type_param_scope: HashMap::new(),
            current_fn_name: String::new(),
            call_resolutions: HashMap::new(),
            struct_resolutions: HashMap::new(),
            enum_resolutions: HashMap::new(),
            mono_fn_bodies: HashMap::new(),
            mono_type_args: HashMap::new(),
            dyn_call_resolutions: HashMap::new(),
            item_def_spans: HashMap::new(),
            symbol_table: SymbolTable::new(),
        }
    }

    pub fn require_main(mut self, require: bool) -> Self {
        self.require_main = require;
        self
    }

    /// Returns true if the accessing code is in a different file than the item's definition.
    pub(super) fn is_cross_module_access(&self, access_span: Span, source_file_id: FileId) -> bool {
        access_span.file_id != source_file_id
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

        // Populate file_symbols for autocomplete
        for (name, sig) in &self.functions {
            if !name.contains("__") && sig.kind == FunctionKind::UserDefined {
                self.symbol_table.file_symbols.push((
                    name.clone(),
                    SymbolKind::Function,
                    Some(sig.return_type),
                    sig.def_span,
                ));
            }
        }
        for (name, &ty) in &self.structs {
            let span = self
                .item_def_spans
                .get(name)
                .copied()
                .unwrap_or(Span::dummy());
            self.symbol_table
                .file_symbols
                .push((name.clone(), SymbolKind::Struct, Some(ty), span));
        }
        for (name, &ty) in &self.enums {
            let span = self
                .item_def_spans
                .get(name)
                .copied()
                .unwrap_or(Span::dummy());
            self.symbol_table
                .file_symbols
                .push((name.clone(), SymbolKind::Enum, Some(ty), span));
        }
        for (name, &ty) in &self.interfaces {
            let span = self
                .item_def_spans
                .get(name)
                .copied()
                .unwrap_or(Span::dummy());
            self.symbol_table.file_symbols.push((
                name.clone(),
                SymbolKind::Interface,
                Some(ty),
                span,
            ));
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
            interface_impls: self.interface_impls,
            interface_method_defs: self.interface_method_defs,
            dyn_call_resolutions: self.dyn_call_resolutions,
            symbol_table: self.symbol_table,
            item_def_spans: self.item_def_spans,
        };
        (checked, self.diagnostics)
    }
}
