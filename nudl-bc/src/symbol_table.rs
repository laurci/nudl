use std::collections::HashMap;

use nudl_core::span::{FileId, Span};
use nudl_core::types::TypeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    EnumVariant,
    Interface,
    Field,
    LocalVariable,
    Parameter,
    TypeAlias,
}

#[derive(Debug, Clone)]
pub struct DefinitionInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub def_span: Span,
    pub type_id: Option<TypeId>,
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// usage_span -> definition info (for go-to-definition)
    pub definitions: HashMap<Span, DefinitionInfo>,
    /// expr_span -> TypeId (for hover)
    pub expr_types: HashMap<Span, TypeId>,
    /// top-level symbols for autocomplete: (name, kind, type_id, def_span)
    pub file_symbols: Vec<(String, SymbolKind, Option<TypeId>, Span)>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            expr_types: HashMap::new(),
            file_symbols: Vec::new(),
        }
    }

    pub fn record_definition(&mut self, usage_span: Span, info: DefinitionInfo) {
        if !usage_span.is_empty() {
            self.definitions.insert(usage_span, info);
        }
    }

    pub fn record_expr_type(&mut self, span: Span, ty: TypeId) {
        if !span.is_empty() {
            self.expr_types.insert(span, ty);
        }
    }

    /// Find the definition for a given file position (byte offset).
    /// Searches for the narrowest span containing the offset.
    pub fn definition_at(&self, file_id: FileId, offset: u32) -> Option<&DefinitionInfo> {
        let mut best: Option<(&Span, &DefinitionInfo)> = None;
        for (span, info) in &self.definitions {
            if span.file_id == file_id && span.start <= offset && offset < span.end {
                if let Some((best_span, _)) = best {
                    if span.len() < best_span.len() {
                        best = Some((span, info));
                    }
                } else {
                    best = Some((span, info));
                }
            }
        }
        best.map(|(_, info)| info)
    }

    /// Find the type for a given file position (byte offset).
    /// Searches for the narrowest span containing the offset.
    pub fn type_at(&self, file_id: FileId, offset: u32) -> Option<TypeId> {
        let mut best: Option<(u32, TypeId)> = None;
        for (span, &ty) in &self.expr_types {
            if span.file_id == file_id && span.start <= offset && offset < span.end {
                let len = span.len();
                if best.map_or(true, |(best_len, _)| len < best_len) {
                    best = Some((len, ty));
                }
            }
        }
        best.map(|(_, ty)| ty)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}
