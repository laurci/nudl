mod calls;
mod context;
mod control_flow;
mod expressions;
mod statements;
mod type_resolve;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use nudl_core::intern::StringInterner;
use nudl_core::span::Span;
use nudl_core::types::{TypeInterner, TypeKind};

use nudl_ast::ast::*;

use crate::checker::{CheckedModule, FunctionSig};
use crate::ir::*;
use crate::scoped_locals::ScopedLocals;

pub use context::FunctionLowerCtx;

/// Lowers AST to SSA bytecode. Consumes CheckedModule for function signatures.
pub struct Lowerer {
    pub(super) interner: StringInterner,
    pub(super) types: TypeInterner,
    pub(super) function_sigs: HashMap<String, FunctionSig>,
    pub(super) struct_defs: HashMap<String, nudl_core::types::TypeId>,
    pub(super) functions: Vec<Function>,
    pub(super) string_constants: Vec<String>,
    pub(super) next_function_id: u32,
    /// Default parameter expressions indexed by function name.
    /// Populated before lowering so call sites can fill in missing args.
    pub(super) param_defaults: HashMap<String, Vec<Option<SpannedExpr>>>,
}

impl Lowerer {
    pub fn new(checked: CheckedModule) -> Self {
        Self {
            interner: StringInterner::new(),
            types: checked.types,
            function_sigs: checked.functions,
            struct_defs: checked.structs,
            functions: Vec::new(),
            string_constants: Vec::new(),
            next_function_id: 0,
            param_defaults: HashMap::new(),
        }
    }

    /// Collect default parameter expressions from all functions in the module.
    fn collect_defaults(module: &Module) -> HashMap<String, Vec<Option<SpannedExpr>>> {
        fn collect_fn_defaults(
            name: &str,
            params: &[Param],
            defaults: &mut HashMap<String, Vec<Option<SpannedExpr>>>,
        ) {
            let has_any_default = params.iter().any(|p| p.default_value.is_some());
            if has_any_default {
                let param_defaults: Vec<Option<SpannedExpr>> = params
                    .iter()
                    .map(|p| p.default_value.as_ref().map(|d| (**d).clone()))
                    .collect();
                defaults.insert(name.to_string(), param_defaults);
            }
        }

        let mut defaults = HashMap::new();
        for item in &module.items {
            match &item.node {
                Item::FnDef { name, params, .. } => {
                    collect_fn_defaults(name, params, &mut defaults);
                }
                Item::ImplBlock {
                    type_name, methods, ..
                } => {
                    for method_item in methods {
                        if let Item::FnDef {
                            name: method_name,
                            params,
                            ..
                        } = &method_item.node
                        {
                            let mangled_name = format!("{}__{}", type_name, method_name);
                            collect_fn_defaults(&mangled_name, params, &mut defaults);
                        }
                    }
                }
                _ => {}
            }
        }
        defaults
    }

    pub fn lower(mut self, module: &Module) -> Program {
        let mut entry_function = None;
        self.param_defaults = Self::collect_defaults(module);

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

        // Pass 2: Lower user-defined functions (including methods from impl blocks)
        for item in &module.items {
            match &item.node {
                Item::FnDef {
                    name, params, body, ..
                } => {
                    let func = self.lower_function(name, params, body);
                    if name == "main" {
                        entry_function = Some(func.id);
                    }
                    self.functions.push(func);
                }
                Item::ImplBlock {
                    type_name, methods, ..
                } => {
                    for method_item in methods {
                        if let Item::FnDef {
                            name: method_name,
                            params,
                            body,
                            ..
                        } = &method_item.node
                        {
                            let mangled_name = format!("{}__{}", type_name, method_name);
                            let func = self.lower_function(&mangled_name, params, body);
                            self.functions.push(func);
                        }
                    }
                }
                _ => {}
            }
        }

        Program {
            functions: self.functions,
            string_constants: self.string_constants,
            entry_function,
            extern_libs: vec!["System".into()],
            interner: self.interner,
            types: self.types,
            source_map: None,
        }
    }

    fn lower_extern_function(&mut self, name: &str) -> Function {
        let id = FunctionId(self.next_function_id);
        self.next_function_id += 1;
        let name_sym = self.interner.intern(name);

        let sig = self.function_sigs.get(name).unwrap().clone();

        let params: Vec<(nudl_core::intern::Symbol, nudl_core::types::TypeId)> = sig
            .params
            .iter()
            .map(|(pname, pty)| (self.interner.intern(pname), *pty))
            .collect();

        Function {
            id,
            name: name_sym,
            params,
            return_type: sig.return_type,
            blocks: vec![],
            register_count: 0,
            register_types: vec![],
            is_extern: true,
            extern_symbol: Some(name.to_string()),
            span: Span::dummy(),
        }
    }

    fn lower_function(
        &mut self,
        name: &str,
        params: &[Param],
        body: &nudl_core::span::Spanned<Block>,
    ) -> Function {
        let id = FunctionId(self.next_function_id);
        self.next_function_id += 1;
        let name_sym = self.interner.intern(name);

        let sig = self.function_sigs.get(name).unwrap().clone();

        let ir_params: Vec<(nudl_core::intern::Symbol, nudl_core::types::TypeId)> = sig
            .params
            .iter()
            .map(|(pname, pty)| (self.interner.intern(pname), *pty))
            .collect();

        // Build locals map from params: param[i].name → Register(i)
        let mut locals = ScopedLocals::<Register>::new();
        let mut next_register = 0u32;
        let mut register_types = Vec::new();
        for param in params {
            locals.insert(param.name.clone(), Register(next_register));
            next_register += 1;
        }

        // Track param types for callee-release and initialize register_types for params
        let mut local_types = ScopedLocals::<nudl_core::types::TypeId>::new();
        for (pname, pty) in sig.params.iter() {
            local_types.insert(pname.clone(), *pty);
            register_types.push(*pty);
        }

        let mut ctx = FunctionLowerCtx {
            blocks: Vec::new(),
            current_block_id: BlockId(0),
            current_instructions: Vec::new(),
            current_spans: Vec::new(),
            current_span: body.span,
            next_block_id: 1,
            next_register,
            locals,
            local_types,
            register_types,
            string_constants: &mut self.string_constants,
            interner: &mut self.interner,
            function_sigs: &self.function_sigs,
            struct_defs: &self.struct_defs,
            types: &mut self.types,
            loop_stack: Vec::new(),
            param_defaults: &self.param_defaults,
        };

        // Lower body — returns the register holding the result
        let result_reg = ctx.lower_block_expr(&body.node);

        // Callee-release: emit Release for reference-typed params at function exit
        for (i, (_pname, pty)) in sig.params.iter().enumerate() {
            let param_reg = Register(i as u32);
            if ctx.types.is_struct(*pty) {
                ctx.push_inst(Instruction::Release(param_reg, Some(*pty)));
            } else if let TypeKind::FixedArray { element, length } = ctx.types.resolve(*pty) {
                let elem = *element;
                let len = *length;
                if ctx.types.is_reference_type(elem) {
                    for idx in 0..len {
                        let idx_reg = ctx.alloc_register();
                        ctx.push_inst(Instruction::Const(idx_reg, ConstValue::I32(idx as i32)));
                        let elem_reg = ctx.alloc_register();
                        ctx.push_inst(Instruction::IndexLoad(elem_reg, param_reg, idx_reg, elem));
                        ctx.push_inst(Instruction::Release(elem_reg, Some(elem)));
                    }
                }
            }
        }

        // Finish the last block with a return
        ctx.finish_block(Terminator::Return(result_reg));

        let register_count = ctx.next_register;
        let register_types = ctx.register_types;
        let blocks = ctx.blocks;

        Function {
            id,
            name: name_sym,
            params: ir_params,
            return_type: sig.return_type,
            blocks,
            register_count,
            register_types,
            is_extern: false,
            extern_symbol: None,
            span: body.span,
        }
    }
}

pub(super) fn parse_int_const(s: &str, suffix: Option<IntSuffix>) -> ConstValue {
    let clean: String = s.chars().filter(|&c| c != '_').collect();
    let (digits, radix) = if clean.starts_with("0x") || clean.starts_with("0X") {
        (&clean[2..], 16)
    } else if clean.starts_with("0o") || clean.starts_with("0O") {
        (&clean[2..], 8)
    } else if clean.starts_with("0b") || clean.starts_with("0B") {
        (&clean[2..], 2)
    } else {
        (clean.as_str(), 10)
    };

    match suffix {
        Some(IntSuffix::U8) | Some(IntSuffix::U16) | Some(IntSuffix::U32)
        | Some(IntSuffix::U64) => {
            let val = u64::from_str_radix(digits, radix).unwrap_or(0);
            ConstValue::U64(val)
        }
        Some(IntSuffix::I64) => {
            let val = i64::from_str_radix(digits, radix).unwrap_or(0);
            ConstValue::I64(val)
        }
        Some(IntSuffix::I8) | Some(IntSuffix::I16) | Some(IntSuffix::I32) | None => {
            let val = i64::from_str_radix(digits, radix).unwrap_or(0);
            ConstValue::I32(val as i32)
        }
    }
}

pub(super) struct LoopContext {
    pub(super) label: Option<String>,
    pub(super) continue_block: BlockId,
    pub(super) break_block: BlockId,
    pub(super) pre_loop_locals: ScopedLocals<Register>,
}
