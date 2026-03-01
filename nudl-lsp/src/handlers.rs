use std::collections::HashMap;

use tower_lsp::lsp_types::request::GotoImplementationResponse;
use tower_lsp::lsp_types::*;

use nudl_bc::symbol_table::SymbolKind;
use nudl_core::source::SourceMap;
use nudl_core::span::FileId;
use nudl_core::types::{TypeId, TypeInterner, TypeKind};

use crate::server::FileCheckResult;

/// Convert an LSP Position to a byte offset within the file.
pub fn position_to_offset(
    source_map: &SourceMap,
    file_id: FileId,
    position: Position,
) -> Option<u32> {
    let file = source_map.get_file(file_id);
    let line_offsets = file.line_offsets();
    let line = position.line as usize;
    if line >= line_offsets.len() {
        return None;
    }
    let line_start = line_offsets[line] as u32;
    Some(line_start + position.character)
}

/// Convert a span to an LSP Range.
fn span_to_range(source_map: &SourceMap, span: nudl_core::span::Span) -> Range {
    let file = source_map.get_file(span.file_id);
    let (start_line, start_col) = file.line_col(span.start);
    let (end_line, end_col) = file.line_col(span.end.min(file.content.len() as u32));
    Range {
        start: Position::new(start_line - 1, start_col - 1),
        end: Position::new(end_line - 1, end_col - 1),
    }
}

/// Handle go-to-definition request.
pub fn handle_goto_definition(
    result: &FileCheckResult,
    position: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = position_to_offset(&result.source_map, result.file_id, position)?;
    let def_info = result.symbol_table.definition_at(result.file_id, offset)?;

    // Builtins have dummy spans — no navigable target
    if def_info.def_span.is_empty() {
        return None;
    }

    let def_file = result.source_map.get_file(def_info.def_span.file_id);
    let uri = Url::from_file_path(&def_file.path).ok()?;
    let range = span_to_range(&result.source_map, def_info.def_span);

    Some(GotoDefinitionResponse::Scalar(Location { uri, range }))
}

/// Format a type for display in hover.
fn format_type_hover(types: &TypeInterner, ty: TypeId) -> String {
    types.type_display_name(ty)
}

/// Format a function signature for hover display.
fn format_fn_sig_hover(
    types: &TypeInterner,
    name: &str,
    params: &[(String, TypeId)],
    return_type: TypeId,
) -> String {
    let param_strs: Vec<String> = params
        .iter()
        .map(|(pname, pty)| format!("{}: {}", pname, types.type_display_name(*pty)))
        .collect();
    let ret = types.type_display_name(return_type);
    let is_unit = matches!(
        types.resolve(return_type),
        TypeKind::Primitive(nudl_core::types::PrimitiveType::Unit)
    );
    if is_unit {
        format!("fn {}({})", name, param_strs.join(", "))
    } else {
        format!("fn {}({}) -> {}", name, param_strs.join(", "), ret)
    }
}

/// Handle hover request.
pub fn handle_hover(result: &FileCheckResult, position: Position) -> Option<Hover> {
    let offset = position_to_offset(&result.source_map, result.file_id, position)?;

    // First try definition_at for rich info (function signature, struct def, etc.)
    if let Some(def_info) = result.symbol_table.definition_at(result.file_id, offset) {
        match def_info.kind {
            SymbolKind::Function | SymbolKind::Method => {
                // Look up the full function signature
                let fn_name = &def_info.name;
                // Try direct lookup or mangled name
                if let Some(sig) = result.functions.get(fn_name) {
                    let sig_str =
                        format_fn_sig_hover(&result.types, fn_name, &sig.params, sig.return_type);
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```nudl\n{}\n```", sig_str),
                        }),
                        range: None,
                    });
                }
                // Fall through to type display
            }
            SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Interface => {
                if let Some(type_id) = def_info.type_id {
                    let type_str = format_type_hover(&result.types, type_id);
                    let kind_str = match def_info.kind {
                        SymbolKind::Struct => "struct",
                        SymbolKind::Enum => "enum",
                        SymbolKind::Interface => "interface",
                        _ => "type",
                    };
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```nudl\n{} {}\n```", kind_str, type_str),
                        }),
                        range: None,
                    });
                }
            }
            _ => {}
        }

        // Show type for any definition
        if let Some(type_id) = def_info.type_id {
            let type_str = format_type_hover(&result.types, type_id);
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```nudl\n{}: {}\n```", def_info.name, type_str),
                }),
                range: None,
            });
        }
    }

    // Fall back to type_at for expression types
    if let Some(ty) = result.symbol_table.type_at(result.file_id, offset) {
        let type_str = format_type_hover(&result.types, ty);
        if type_str != "<error>" {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```nudl\n{}\n```", type_str),
                }),
                range: None,
            });
        }
    }

    None
}

/// nudl language keywords for autocomplete.
const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "const",
    "if",
    "else",
    "while",
    "for",
    "in",
    "loop",
    "break",
    "continue",
    "return",
    "match",
    "struct",
    "enum",
    "impl",
    "interface",
    "import",
    "pub",
    "true",
    "false",
    "as",
    "defer",
    "extern",
    "type",
    "comptime",
    "dyn",
    "where",
    "self",
];

/// Extract the identifier before the cursor in the source text.
fn ident_before_offset(source: &str, offset: u32) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut end = offset as usize;
    if end > bytes.len() {
        end = bytes.len();
    }
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    if start == end {
        None
    } else {
        Some(&source[start..end])
    }
}

/// Handle completion request.
pub fn handle_completion(
    result: &FileCheckResult,
    position: Position,
    trigger_char: Option<&str>,
    live_source: Option<&str>,
) -> Option<CompletionResponse> {
    let offset = position_to_offset(&result.source_map, result.file_id, position)?;

    // Use live source (from did_change) if available, otherwise fall back to cached source.
    // This is important for trigger-based completions (`.`, `::`) because the cached source
    // may not contain the trigger character yet (if re-check failed due to parse errors).
    let file = result.source_map.get_file(result.file_id);
    let source = live_source.unwrap_or(&file.content);

    if trigger_char == Some(".") {
        // Dot completions: find type of expression before the dot
        return handle_dot_completion(result, offset);
    }

    if trigger_char == Some(":") {
        // Check for :: (double colon) completions
        let off = offset as usize;
        // offset is right after the second ':'. So source[off-1] = ':', source[off-2] = ':'
        if off >= 2 && source.as_bytes().get(off - 2) == Some(&b':') {
            // We have "::" — get the type name before the first ':'
            let before_colons = off - 2;
            if before_colons == 0 {
                return None;
            }
            if let Some(type_name) = ident_before_offset(source, before_colons as u32) {
                return handle_double_colon_completion(result, type_name);
            }
        }
        return None;
    }

    // Global completions
    let mut items = Vec::new();

    // Local variables and parameters in scope at the cursor position
    {
        let mut seen_locals = std::collections::HashSet::new();
        for (span, info) in &result.symbol_table.definitions {
            if span.file_id != result.file_id {
                continue;
            }
            match info.kind {
                SymbolKind::LocalVariable | SymbolKind::Parameter => {}
                _ => continue,
            }
            // Filter out `self` — it's not useful as a completion
            if info.name == "self" {
                continue;
            }
            // Only include if defined before the cursor
            if info.def_span.file_id == result.file_id && info.def_span.start <= offset {
                if seen_locals.insert(info.name.clone()) {
                    let detail = info.type_id.map(|ty| format_type_hover(&result.types, ty));
                    items.push(CompletionItem {
                        label: info.name.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail,
                        sort_text: Some(format!("0{}", info.name)),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Functions (skip mangled/internal names)
    for (name, sig) in &result.functions {
        if name.contains("__") || name.starts_with("_") {
            continue;
        }
        if name.contains("$") {
            continue; // skip monomorphized names
        }
        let detail = format_fn_sig_hover(&result.types, name, &sig.params, sig.return_type);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            ..Default::default()
        });
    }

    // Structs
    for (name, &ty) in &result.structs {
        if name.contains("$") {
            continue; // skip monomorphized names
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some(format_type_hover(&result.types, ty)),
            ..Default::default()
        });
    }

    // Enums
    for (name, &ty) in &result.enums {
        if name.contains("$") {
            continue; // skip monomorphized names
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some(format_type_hover(&result.types, ty)),
            ..Default::default()
        });
    }

    // Interfaces
    for (name, &ty) in &result.interfaces {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::INTERFACE),
            detail: Some(format_type_hover(&result.types, ty)),
            ..Default::default()
        });
    }

    // Keywords
    for &kw in KEYWORDS {
        items.push(CompletionItem {
            label: kw.into(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    // Import suggestions from project symbols
    for (_uri, symbols) in &result.project_symbols {
        for sym in symbols {
            // Only suggest public symbols not already available
            if !sym.is_pub {
                continue;
            }
            // Skip if already in scope
            if result.functions.contains_key(&sym.name)
                || result.structs.contains_key(&sym.name)
                || result.enums.contains_key(&sym.name)
                || result.interfaces.contains_key(&sym.name)
            {
                continue;
            }

            let kind = match sym.kind {
                SymbolKind::Function => CompletionItemKind::FUNCTION,
                SymbolKind::Struct => CompletionItemKind::STRUCT,
                SymbolKind::Enum => CompletionItemKind::ENUM,
                SymbolKind::Interface => CompletionItemKind::INTERFACE,
                _ => CompletionItemKind::TEXT,
            };

            let import_path_str = sym.import_path.join("::");
            items.push(CompletionItem {
                label: sym.name.clone(),
                kind: Some(kind),
                detail: Some(format!("import {}", import_path_str)),
                additional_text_edits: Some(vec![TextEdit {
                    range: Range {
                        start: Position::new(0, 0),
                        end: Position::new(0, 0),
                    },
                    new_text: format!("import {};\n", import_path_str),
                }]),
                ..Default::default()
            });
        }
    }

    Some(CompletionResponse::Array(items))
}

/// Handle `::` completions for enum variants and static methods.
fn handle_double_colon_completion(
    result: &FileCheckResult,
    type_name: &str,
) -> Option<CompletionResponse> {
    let mut items = Vec::new();

    // Enum variants
    if let Some(&enum_ty) = result.enums.get(type_name) {
        if let TypeKind::Enum { variants, .. } = result.types.resolve(enum_ty).clone() {
            for variant in &variants {
                let detail = if variant.fields.is_empty() {
                    type_name.to_string()
                } else {
                    let field_strs: Vec<String> = variant
                        .fields
                        .iter()
                        .map(|(fname, fty)| {
                            if fname.starts_with("_") {
                                format_type_hover(&result.types, *fty)
                            } else {
                                format!("{}: {}", fname, format_type_hover(&result.types, *fty))
                            }
                        })
                        .collect();
                    format!("{}::{}({})", type_name, variant.name, field_strs.join(", "))
                };
                items.push(CompletionItem {
                    label: variant.name.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some(detail),
                    ..Default::default()
                });
            }
        }
    }

    // Static methods (TypeName__method where first param is NOT self)
    let prefix = format!("{type_name}__");
    for (mangled_name, sig) in &result.functions {
        if let Some(method_name) = mangled_name.strip_prefix(&prefix) {
            if method_name.contains("__") {
                continue;
            }
            // Static methods: first param name is NOT "self"
            let is_static = sig
                .params
                .first()
                .map_or(true, |(pname, _)| pname != "self");
            if is_static {
                let detail =
                    format_fn_sig_hover(&result.types, method_name, &sig.params, sig.return_type);
                items.push(CompletionItem {
                    label: method_name.to_string(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some(detail),
                    ..Default::default()
                });
            }
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// Handle dot-triggered completions (fields and methods).
fn handle_dot_completion(result: &FileCheckResult, offset: u32) -> Option<CompletionResponse> {
    // Find the type of the expression before the dot
    // offset points right after the dot, so look at offset-2 (before the dot)
    let before_dot = if offset > 1 { offset - 2 } else { return None };
    let ty = result.symbol_table.type_at(result.file_id, before_dot)?;

    let mut items = Vec::new();

    match result.types.resolve(ty).clone() {
        TypeKind::Struct { name, fields, .. } => {
            // Fields
            for (field_name, field_ty) in &fields {
                items.push(CompletionItem {
                    label: field_name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format_type_hover(&result.types, *field_ty)),
                    ..Default::default()
                });
            }
            // Methods (TypeName__method)
            add_method_completions(&name, &result.functions, &result.types, &mut items);
        }
        TypeKind::Enum { name, .. } => {
            add_method_completions(&name, &result.functions, &result.types, &mut items);
        }
        TypeKind::String => {
            add_method_completions("string", &result.functions, &result.types, &mut items);
            // Built-in string methods
            items.push(CompletionItem {
                label: "len".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some("fn len() -> i64".into()),
                ..Default::default()
            });
        }
        TypeKind::DynamicArray { element } => {
            let elem_name = format_type_hover(&result.types, element);
            items.push(CompletionItem {
                label: "push".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn push(value: {})", elem_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "pop".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn pop() -> {}", elem_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "remove".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn remove(index: i64) -> {}", elem_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "len".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some("fn len() -> i64".into()),
                ..Default::default()
            });
        }
        TypeKind::Map { key, value } => {
            let key_name = format_type_hover(&result.types, key);
            let val_name = format_type_hover(&result.types, value);
            items.push(CompletionItem {
                label: "insert".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn insert(key: {}, value: {})", key_name, val_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "get".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn get(key: {}) -> Option<{}>", key_name, val_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "contains_key".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn contains_key(key: {}) -> bool", key_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "remove".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("fn remove(key: {}) -> bool", key_name)),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "len".into(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some("fn len() -> i64".into()),
                ..Default::default()
            });
        }
        TypeKind::DynInterface { name } => {
            add_method_completions(&name, &result.functions, &result.types, &mut items);
        }
        _ => {
            // Try primitive types with impl methods
            let type_name = format_type_hover(&result.types, ty);
            add_method_completions(&type_name, &result.functions, &result.types, &mut items);
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// Resolve a definition span to a canonical (file_path, start, end) triple for cross-file comparison.
/// Each check_document creates its own SourceMap with independent FileId numbering, so we
/// can't compare Spans across different check results directly.
fn resolve_span_location(
    source_map: &SourceMap,
    span: nudl_core::span::Span,
) -> Option<(String, u32, u32)> {
    if span.is_empty() {
        return None;
    }
    let file = source_map.get_file(span.file_id);
    Some((
        file.path.to_string_lossy().to_string(),
        span.start,
        span.end,
    ))
}

/// Find the definition span when the cursor is on a definition site itself
/// (not a usage site). Checks file_symbols and function signatures.
fn definition_span_at(result: &FileCheckResult, offset: u32) -> Option<nudl_core::span::Span> {
    // Check file_symbols (top-level structs, enums, interfaces, user-defined functions)
    for (_name, _kind, _type_id, def_span) in &result.symbol_table.file_symbols {
        if def_span.file_id == result.file_id && def_span.start <= offset && offset < def_span.end {
            return Some(*def_span);
        }
    }
    // Check all function signatures (includes methods that file_symbols may skip)
    for (_name, sig) in &result.functions {
        if sig.def_span.file_id == result.file_id
            && sig.def_span.start <= offset
            && offset < sig.def_span.end
        {
            return Some(sig.def_span);
        }
    }
    None
}

/// Handle find-references request.
/// Searches all cached files for usages that point to the same definition span.
pub fn handle_references(
    result: &FileCheckResult,
    position: Position,
    include_declaration: bool,
    file_cache: &HashMap<Url, FileCheckResult>,
) -> Option<Vec<Location>> {
    let offset = position_to_offset(&result.source_map, result.file_id, position)?;

    // Determine the target definition span.
    // Case 1: cursor is on a usage site — definition_at gives us the def_span
    // Case 2: cursor is on a definition site — find it via file_symbols / function sigs
    let target_def_span =
        if let Some(def_info) = result.symbol_table.definition_at(result.file_id, offset) {
            def_info.def_span
        } else {
            definition_span_at(result, offset)?
        };

    if target_def_span.is_empty() {
        return None;
    }

    // Resolve to a canonical location for cross-file comparison
    let target_location = resolve_span_location(&result.source_map, target_def_span)?;

    let mut locations = Vec::new();

    // Optionally include the declaration itself
    if include_declaration {
        let file = result.source_map.get_file(target_def_span.file_id);
        if let Ok(def_uri) = Url::from_file_path(&file.path) {
            let range = span_to_range(&result.source_map, target_def_span);
            locations.push(Location {
                uri: def_uri,
                range,
            });
        }
    }

    // Search all cached files for usages pointing to the same definition
    for (uri, cached) in file_cache {
        for (usage_span, info) in &cached.symbol_table.definitions {
            // Compare by resolved file path + byte offsets (not FileId, which is per-SourceMap)
            if let Some(loc) = resolve_span_location(&cached.source_map, info.def_span) {
                if loc == target_location {
                    let range = span_to_range(&cached.source_map, *usage_span);
                    let location = Location {
                        uri: uri.clone(),
                        range,
                    };
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

/// Handle go-to-implementation request.
/// For interface names: finds all types that implement the interface.
/// For interface methods: finds the concrete method implementations in impl blocks.
pub fn handle_goto_implementation(
    result: &FileCheckResult,
    position: Position,
    file_cache: &HashMap<Url, FileCheckResult>,
) -> Option<GotoImplementationResponse> {
    let offset = position_to_offset(&result.source_map, result.file_id, position)?;

    // Check if cursor is on a method inside an interface definition
    if let Some((iface_name, method_name)) = find_interface_method_at(result, offset) {
        return goto_method_implementations(&iface_name, &method_name, file_cache);
    }

    // Try usage site first (cursor on an interface name reference)
    let interface_name =
        if let Some(def_info) = result.symbol_table.definition_at(result.file_id, offset) {
            if def_info.kind != SymbolKind::Interface {
                return None;
            }
            def_info.name.clone()
        } else {
            // Cursor might be on the interface definition name itself
            let mut found = None;
            for (name, kind, _type_id, def_span) in &result.symbol_table.file_symbols {
                if *kind == SymbolKind::Interface
                    && def_span.file_id == result.file_id
                    && def_span.start <= offset
                    && offset < def_span.end
                {
                    found = Some(name.clone());
                    break;
                }
            }
            found?
        };

    goto_type_implementations(&interface_name, file_cache)
}

/// Check if offset is on a method name inside an interface definition.
/// Returns (interface_name, method_name) if found.
fn find_interface_method_at(result: &FileCheckResult, offset: u32) -> Option<(String, String)> {
    for (iface_name, methods) in &result.interface_method_defs {
        for method in methods {
            if method.span.file_id == result.file_id
                && method.span.start <= offset
                && offset < method.span.end
            {
                return Some((iface_name.clone(), method.name.clone()));
            }
        }
    }
    None
}

/// Find all types implementing the given interface and return their definition locations.
fn goto_type_implementations(
    interface_name: &str,
    file_cache: &HashMap<Url, FileCheckResult>,
) -> Option<GotoImplementationResponse> {
    let mut locations = Vec::new();

    for (_uri, cached) in file_cache {
        if let Some(impl_types) = cached.interface_impls.get(interface_name) {
            for type_name in impl_types {
                if let Some(&def_span) = cached.item_def_spans.get(type_name) {
                    if def_span.is_empty() {
                        continue;
                    }
                    let file = cached.source_map.get_file(def_span.file_id);
                    if let Ok(type_uri) = Url::from_file_path(&file.path) {
                        let range = span_to_range(&cached.source_map, def_span);
                        let loc = Location {
                            uri: type_uri,
                            range,
                        };
                        if !locations.contains(&loc) {
                            locations.push(loc);
                        }
                    }
                }
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(GotoImplementationResponse::Array(locations))
    }
}

/// Find concrete implementations of a specific interface method (TypeName__methodName).
fn goto_method_implementations(
    interface_name: &str,
    method_name: &str,
    file_cache: &HashMap<Url, FileCheckResult>,
) -> Option<GotoImplementationResponse> {
    let mut locations = Vec::new();

    for (_uri, cached) in file_cache {
        if let Some(impl_types) = cached.interface_impls.get(interface_name) {
            for type_name in impl_types {
                let mangled = format!("{}__{}", type_name, method_name);
                // Try item_def_spans first (method span from collect pass)
                let def_span = cached
                    .item_def_spans
                    .get(&mangled)
                    .copied()
                    .or_else(|| cached.functions.get(&mangled).map(|sig| sig.def_span));
                if let Some(def_span) = def_span {
                    if def_span.is_empty() {
                        continue;
                    }
                    let file = cached.source_map.get_file(def_span.file_id);
                    if let Ok(type_uri) = Url::from_file_path(&file.path) {
                        let range = span_to_range(&cached.source_map, def_span);
                        let loc = Location {
                            uri: type_uri,
                            range,
                        };
                        if !locations.contains(&loc) {
                            locations.push(loc);
                        }
                    }
                }
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(GotoImplementationResponse::Array(locations))
    }
}

/// Add instance method completions for a given type name by looking for TypeName__method functions.
/// Only includes methods whose first parameter is `self` (instance methods, not static).
fn add_method_completions(
    type_name: &str,
    functions: &std::collections::HashMap<String, nudl_bc::checker::FunctionSig>,
    types: &TypeInterner,
    items: &mut Vec<CompletionItem>,
) {
    let prefix = format!("{type_name}__");
    for (mangled_name, sig) in functions {
        if let Some(method_name) = mangled_name.strip_prefix(&prefix) {
            // Skip internal/double-mangled names
            if method_name.contains("__") {
                continue;
            }
            // Only include instance methods (first param is `self`)
            let is_instance = sig.params.first().is_some_and(|(pname, _)| pname == "self");
            if !is_instance {
                continue;
            }
            // Show params without `self`
            let detail = format_fn_sig_hover(
                types,
                method_name,
                &sig.params[1..].to_vec(),
                sig.return_type,
            );
            items.push(CompletionItem {
                label: method_name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(detail),
                ..Default::default()
            });
        }
    }
}
