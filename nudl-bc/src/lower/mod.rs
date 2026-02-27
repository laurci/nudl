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

/// Information about a closure that needs to be lowered as a separate function.
pub(super) struct PendingClosure {
    /// The function ID assigned to the closure thunk
    pub(super) func_id: FunctionId,
    /// Names of captured variables (in order they appear in the capture struct)
    pub(super) capture_names: Vec<String>,
    /// Types of captured variables
    pub(super) capture_types: Vec<nudl_core::types::TypeId>,
    /// The closure's parameter names and types
    pub(super) params: Vec<(String, nudl_core::types::TypeId)>,
    /// The closure body AST
    pub(super) body: SpannedExpr,
    /// Return type
    pub(super) return_type: nudl_core::types::TypeId,
    /// Span
    pub(super) span: Span,
}

/// Lowers AST to SSA bytecode. Consumes CheckedModule for function signatures.
pub struct Lowerer {
    pub(super) interner: StringInterner,
    pub(super) types: TypeInterner,
    pub(super) function_sigs: HashMap<String, FunctionSig>,
    pub(super) struct_defs: HashMap<String, nudl_core::types::TypeId>,
    pub(super) enum_defs: HashMap<String, nudl_core::types::TypeId>,
    pub(super) functions: Vec<Function>,
    pub(super) string_constants: Vec<String>,
    pub(super) next_function_id: u32,
    /// Default parameter expressions indexed by function name.
    /// Populated before lowering so call sites can fill in missing args.
    pub(super) param_defaults: HashMap<String, Vec<Option<SpannedExpr>>>,
    /// Closures that need to be lowered as separate functions after the current function.
    pub(super) pending_closures: Vec<PendingClosure>,
}

impl Lowerer {
    pub fn new(checked: CheckedModule) -> Self {
        Self {
            interner: StringInterner::new(),
            types: checked.types,
            function_sigs: checked.functions,
            struct_defs: checked.structs,
            enum_defs: checked.enums,
            functions: Vec::new(),
            string_constants: Vec::new(),
            next_function_id: 0,
            param_defaults: HashMap::new(),
            pending_closures: Vec::new(),
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
                Item::Import { .. } => {
                    // Imports handled at pipeline level
                }
                _ => {}
            }
        }

        // Pass 3: Lower pending closures (generated during function lowering)
        while !self.pending_closures.is_empty() {
            let closures = std::mem::take(&mut self.pending_closures);
            for closure in closures {
                let func = self.lower_closure_thunk(closure);
                self.functions.push(func);
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
            enum_defs: &self.enum_defs,
            types: &mut self.types,
            loop_stack: Vec::new(),
            param_defaults: &self.param_defaults,
            deferred_blocks: Vec::new(),
            pending_closures: &mut self.pending_closures,
            next_function_id: &mut self.next_function_id,
        };

        // Lower body — returns the register holding the result
        let result_reg = ctx.lower_block_expr(&body.node);

        // Emit deferred blocks in LIFO order
        let deferred = std::mem::take(&mut ctx.deferred_blocks);
        for block in deferred.into_iter().rev() {
            ctx.lower_block_expr(&block);
        }

        // Callee-release: emit Release for reference-typed params at function exit
        for (i, (_pname, pty)) in sig.params.iter().enumerate() {
            let param_reg = Register(i as u32);
            if ctx.types.is_reference_type(*pty)
                && !matches!(ctx.types.resolve(*pty), TypeKind::String)
            {
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

    /// Lower a closure thunk function. The first parameter is `__env` (pointer to capture struct).
    /// Subsequent parameters are the closure's declared parameters.
    fn lower_closure_thunk(&mut self, closure: PendingClosure) -> Function {
        let id = closure.func_id;
        let thunk_name = format!("__closure_{}", id.0);
        let name_sym = self.interner.intern(&thunk_name);

        // Build IR params: first is __env (i64 representing pointer), then closure params
        let env_ty = self.types.i64();
        let env_sym = self.interner.intern("__env");
        let mut ir_params: Vec<(nudl_core::intern::Symbol, nudl_core::types::TypeId)> =
            vec![(env_sym, env_ty)];
        for (pname, pty) in &closure.params {
            ir_params.push((self.interner.intern(pname), *pty));
        }

        // Build locals: __env is register 0, params are registers 1..N,
        // then captured vars are loaded from __env
        let mut locals = ScopedLocals::<Register>::new();
        let mut local_types = ScopedLocals::<nudl_core::types::TypeId>::new();
        let mut register_types = Vec::new();
        let mut next_register = 0u32;

        // Register 0: __env
        locals.insert("__env".to_string(), Register(next_register));
        local_types.insert("__env".to_string(), env_ty);
        register_types.push(env_ty);
        next_register += 1;

        // Registers 1..N: closure params
        for (pname, pty) in &closure.params {
            locals.insert(pname.clone(), Register(next_register));
            local_types.insert(pname.clone(), *pty);
            register_types.push(*pty);
            next_register += 1;
        }

        let mut ctx = FunctionLowerCtx {
            blocks: Vec::new(),
            current_block_id: BlockId(0),
            current_instructions: Vec::new(),
            current_spans: Vec::new(),
            current_span: closure.span,
            next_block_id: 1,
            next_register,
            locals,
            local_types,
            register_types,
            string_constants: &mut self.string_constants,
            interner: &mut self.interner,
            function_sigs: &self.function_sigs,
            struct_defs: &self.struct_defs,
            enum_defs: &self.enum_defs,
            types: &mut self.types,
            loop_stack: Vec::new(),
            param_defaults: &self.param_defaults,
            deferred_blocks: Vec::new(),
            pending_closures: &mut self.pending_closures,
            next_function_id: &mut self.next_function_id,
        };

        // Load captured variables from the env pointer
        let env_reg = Register(0);
        for (i, (cap_name, cap_type)) in closure
            .capture_names
            .iter()
            .zip(closure.capture_types.iter())
            .enumerate()
        {
            let cap_reg = ctx.alloc_typed_register(*cap_type);
            // Load from env struct: header(16) + field_index * 8
            ctx.push_inst(Instruction::Load(cap_reg, env_reg, i as u32));
            ctx.locals.insert(cap_name.clone(), cap_reg);
            ctx.local_types.insert(cap_name.clone(), *cap_type);
        }

        // Lower the closure body
        let result_reg = ctx.lower_expr(&closure.body);

        // Finish with return
        ctx.finish_block(Terminator::Return(result_reg));

        let register_count = ctx.next_register;
        let register_types = ctx.register_types;
        let blocks = ctx.blocks;

        Function {
            id,
            name: name_sym,
            params: ir_params,
            return_type: closure.return_type,
            blocks,
            register_count,
            register_types,
            is_extern: false,
            extern_symbol: None,
            span: closure.span,
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
