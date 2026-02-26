use std::collections::HashMap;

use nudl_core::intern::StringInterner;
use nudl_core::span::Span;
use nudl_core::types::{TypeInterner, TypeKind};

use nudl_ast::ast::*;

use crate::checker::{CheckedModule, FunctionKind, FunctionSig};
use crate::ir::*;
use crate::scoped_locals::ScopedLocals;

/// Lowers AST to SSA bytecode. Consumes CheckedModule for function signatures.
pub struct Lowerer {
    interner: StringInterner,
    types: TypeInterner,
    function_sigs: HashMap<String, FunctionSig>,
    struct_defs: HashMap<String, nudl_core::types::TypeId>,
    functions: Vec<Function>,
    string_constants: Vec<String>,
    next_function_id: u32,
    /// Default parameter expressions indexed by function name.
    /// Populated before lowering so call sites can fill in missing args.
    param_defaults: HashMap<String, Vec<Option<SpannedExpr>>>,
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
        fn collect_fn_defaults(name: &str, params: &[Param], defaults: &mut HashMap<String, Vec<Option<SpannedExpr>>>) {
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
                Item::ImplBlock { type_name, methods, .. } => {
                    for method_item in methods {
                        if let Item::FnDef { name: method_name, params, .. } = &method_item.node {
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

fn parse_int_const(s: &str, suffix: Option<IntSuffix>) -> ConstValue {
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
        Some(IntSuffix::U8) | Some(IntSuffix::U16) | Some(IntSuffix::U32) | Some(IntSuffix::U64) => {
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

struct LoopContext {
    label: Option<String>,
    continue_block: BlockId,
    break_block: BlockId,
    pre_loop_locals: ScopedLocals<Register>,
}

struct FunctionLowerCtx<'a> {
    blocks: Vec<BasicBlock>,
    current_block_id: BlockId,
    current_instructions: Vec<Instruction>,
    current_spans: Vec<Span>,
    current_span: Span,
    next_block_id: u32,
    next_register: u32,
    locals: ScopedLocals<Register>,
    /// Track which locals are struct-typed (for Release at scope exit)
    local_types: ScopedLocals<nudl_core::types::TypeId>,
    /// TypeId for each register, indexed by Register.0
    register_types: Vec<nudl_core::types::TypeId>,
    string_constants: &'a mut Vec<String>,
    interner: &'a mut StringInterner,
    function_sigs: &'a HashMap<String, FunctionSig>,
    struct_defs: &'a HashMap<String, nudl_core::types::TypeId>,
    types: &'a mut TypeInterner,
    loop_stack: Vec<LoopContext>,
    /// Default parameter expressions for all functions (for filling in defaults at call sites)
    param_defaults: &'a HashMap<String, Vec<Option<SpannedExpr>>>,
}

impl<'a> FunctionLowerCtx<'a> {
    fn alloc_register(&mut self) -> Register {
        let r = Register(self.next_register);
        self.next_register += 1;
        // Default to i64
        let default_ty = self.types.i64();
        self.register_types.push(default_ty);
        r
    }

    fn alloc_typed_register(&mut self, type_id: nudl_core::types::TypeId) -> Register {
        let r = Register(self.next_register);
        self.next_register += 1;
        self.register_types.push(type_id);
        r
    }

    fn new_block_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    /// Finish the current block with the given terminator and start a new block
    fn finish_block(&mut self, terminator: Terminator) -> BlockId {
        let block = BasicBlock {
            id: self.current_block_id,
            instructions: std::mem::take(&mut self.current_instructions),
            spans: std::mem::take(&mut self.current_spans),
            terminator,
        };
        self.blocks.push(block);
        let old_id = self.current_block_id;
        self.current_block_id = self.new_block_id();
        old_id
    }

    /// Start a specific block (set it as current)
    fn start_block(&mut self, id: BlockId) {
        self.current_block_id = id;
        self.current_instructions.clear();
        self.current_spans.clear();
    }

    /// Push an instruction along with the current span
    fn push_inst(&mut self, inst: Instruction) {
        self.current_instructions.push(inst);
        self.current_spans.push(self.current_span);
    }

    /// Lower a block and return the register holding its value.
    /// Pushes a new scope so variables defined inside are not visible outside.
    fn lower_block_expr(&mut self, block: &Block) -> Register {
        self.locals.push_scope();
        self.local_types.push_scope();
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
        let result = if let Some(tail) = &block.tail_expr {
            self.lower_expr(tail)
        } else {
            let reg = self.alloc_register();
            self.push_inst(Instruction::ConstUnit(reg));
            reg
        };

        // Scope release: emit Release for reference-typed locals defined in this scope
        let scope_types = self.local_types.current_scope_entries();
        for (name, type_id) in &scope_types {
            if let Some(&reg) = self.locals.get(name) {
                if self.types.is_struct(*type_id) {
                    self.push_inst(Instruction::Release(reg, Some(*type_id)));
                } else if let TypeKind::FixedArray { element, length } = self.types.resolve(*type_id) {
                    let elem = *element;
                    let len = *length;
                    if self.types.is_reference_type(elem) {
                        for idx in 0..len {
                            let idx_reg = self.alloc_register();
                            self.push_inst(Instruction::Const(idx_reg, ConstValue::I32(idx as i32)));
                            let elem_reg = self.alloc_register();
                            self.push_inst(Instruction::IndexLoad(elem_reg, reg, idx_reg, elem));
                            self.push_inst(Instruction::Release(elem_reg, Some(elem)));
                        }
                    }
                }
            }
        }

        self.local_types.pop_scope();
        self.locals.pop_scope();
        result
    }

    fn lower_stmt(&mut self, stmt: &nudl_core::span::Spanned<Stmt>) {
        self.current_span = stmt.span;
        match &stmt.node {
            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }
            Stmt::Let { name, value, .. } | Stmt::Const { name, value, .. } => {
                let reg = self.lower_expr(value);
                self.locals.insert(name.clone(), reg);

                // Track typed locals for scope-exit Release, field/index type inference,
                // and float type propagation
                if let Some(type_id) = self.infer_expr_type(value) {
                    self.local_types.insert(name.clone(), type_id);
                }
            }
            Stmt::Item(_) => {} // nested items not supported yet
        }
    }

    fn lower_expr(&mut self, expr: &nudl_core::span::Spanned<Expr>) -> Register {
        self.current_span = expr.span;
        match &expr.node {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(name).cloned() {
                        return match sig.kind {
                            FunctionKind::Builtin => self.lower_builtin_call(name, args),
                            FunctionKind::Extern => self.lower_resolved_call(name, &sig, args, true, 0),
                            FunctionKind::UserDefined => self.lower_resolved_call(name, &sig, args, false, 0),
                        };
                    }
                }
                // Fallback: emit unit
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::MethodCall { object, method, args } => {
                // First, infer receiver type name for mangled lookup
                let type_name = self.infer_receiver_type_name(object);
                if let Some(type_name) = type_name {
                    let mangled_name = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                        // Lower the object (self argument)
                        let self_reg = self.lower_expr(object);
                        return self.lower_method_call(&mangled_name, &sig, self_reg, args);
                    }
                }
                // Fallback
                self.lower_expr(object);
                for arg in args { self.lower_expr(&arg.value); }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::StaticCall { type_name, method, args } => {
                let mangled_name = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                    return self.lower_resolved_call(&mangled_name, &sig, args, false, 0);
                }
                // Fallback
                for arg in args { self.lower_expr(&arg.value); }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
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
                self.push_inst(Instruction::Const(reg, ConstValue::StringLiteral(idx)));
                reg
            }

            Expr::Literal(Literal::Int(s, suffix)) => {
                let const_val = parse_int_const(s, *suffix);
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, const_val));
                reg
            }

            Expr::Literal(Literal::Float(s)) => {
                let val: f64 = s.parse().unwrap_or(0.0);
                let f64_ty = self.types.f64();
                let reg = self.alloc_typed_register(f64_ty);
                self.push_inst(Instruction::Const(reg, ConstValue::F64(val)));
                reg
            }

            Expr::Literal(Literal::Bool(b)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Bool(*b)));
                reg
            }

            Expr::Literal(Literal::Char(c)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Char(*c)));
                reg
            }

            Expr::Ident(name) => {
                if let Some(&reg) = self.locals.get(name) {
                    reg
                } else {
                    // Should have been caught by checker
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                }
            }

            Expr::Return(value) => {
                let ret_reg = if let Some(inner) = value {
                    self.lower_expr(inner)
                } else {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                };
                // Terminate current block with a Return and start a dead block
                // for any subsequent code in the same scope.
                let dead_block = self.new_block_id();
                self.finish_block(Terminator::Return(ret_reg));
                self.start_block(dead_block);
                // Return unit since the code after this is unreachable
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Binary { op, left, right } => {
                // Short-circuit for && and ||
                match op {
                    BinOp::And => return self.lower_short_circuit_and(left, right),
                    BinOp::Or => return self.lower_short_circuit_or(left, right),
                    _ => {}
                }

                let binop_span = self.current_span;
                let lhs = self.lower_expr(left);
                let rhs = self.lower_expr(right);
                self.current_span = binop_span;

                // Check if this is a float operation (propagate lhs type for arithmetic)
                let lhs_type = self.register_types[lhs.0 as usize];
                let is_float = matches!(
                    self.types.resolve(lhs_type),
                    TypeKind::Primitive(p) if p.is_float()
                );

                // Comparisons always produce i64 (bool); arithmetic propagates operand type
                let is_comparison = matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                );

                let dst = if is_float && !is_comparison {
                    self.alloc_typed_register(lhs_type)
                } else {
                    self.alloc_register()
                };

                let inst = match op {
                    BinOp::Add => Instruction::Add(dst, lhs, rhs),
                    BinOp::Sub => Instruction::Sub(dst, lhs, rhs),
                    BinOp::Mul => Instruction::Mul(dst, lhs, rhs),
                    BinOp::Div => Instruction::Div(dst, lhs, rhs),
                    BinOp::Mod => Instruction::Mod(dst, lhs, rhs),
                    BinOp::Shl => Instruction::Shl(dst, lhs, rhs),
                    BinOp::Shr => Instruction::Shr(dst, lhs, rhs),
                    BinOp::BitAnd => Instruction::BitAnd(dst, lhs, rhs),
                    BinOp::BitOr => Instruction::BitOr(dst, lhs, rhs),
                    BinOp::BitXor => Instruction::BitXor(dst, lhs, rhs),
                    BinOp::Eq => Instruction::Eq(dst, lhs, rhs),
                    BinOp::Ne => Instruction::Ne(dst, lhs, rhs),
                    BinOp::Lt => Instruction::Lt(dst, lhs, rhs),
                    BinOp::Le => Instruction::Le(dst, lhs, rhs),
                    BinOp::Gt => Instruction::Gt(dst, lhs, rhs),
                    BinOp::Ge => Instruction::Ge(dst, lhs, rhs),
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.push_inst(inst);
                dst
            }

            Expr::Unary { op, operand } => {
                let src = self.lower_expr(operand);
                let src_type = self.register_types[src.0 as usize];
                let is_float = matches!(
                    self.types.resolve(src_type),
                    TypeKind::Primitive(p) if p.is_float()
                );
                let dst = if is_float && matches!(op, UnaryOp::Neg) {
                    self.alloc_typed_register(src_type)
                } else {
                    self.alloc_register()
                };
                let inst = match op {
                    UnaryOp::Neg => Instruction::Neg(dst, src),
                    UnaryOp::Not => Instruction::Not(dst, src),
                    UnaryOp::BitNot => Instruction::BitNot(dst, src),
                };
                self.push_inst(inst);
                dst
            }

            Expr::StructLiteral { name, fields } => {
                let type_id = self.struct_defs.get(name.as_str()).copied().unwrap();
                let dst = self.alloc_register();
                self.push_inst(Instruction::Alloc(dst, type_id));

                // Resolve field order from the type definition
                let struct_fields = match self.types.resolve(type_id).clone() {
                    nudl_core::types::TypeKind::Struct { fields: f, .. } => f,
                    _ => vec![],
                };

                for (field_name, field_val) in fields {
                    let val_reg = self.lower_expr(field_val);
                    // Find field index in the struct definition
                    let field_idx = struct_fields
                        .iter()
                        .position(|(n, _)| n == field_name)
                        .unwrap() as u32;
                    self.push_inst(Instruction::Store(dst, field_idx, val_reg));
                }
                dst
            }

            Expr::FieldAccess { object, field } => {
                let obj_reg = self.lower_expr(object);
                // Check if this is a tuple field access (.0, .1, etc.)
                if let Ok(idx) = field.parse::<u32>() {
                    let obj_type = self.infer_expr_type(object);
                    if let Some(tid) = obj_type {
                        if self.types.is_tuple(tid) {
                            let dst = self.alloc_register();
                            self.push_inst(Instruction::TupleLoad(dst, obj_reg, idx));
                            return dst;
                        }
                    }
                }
                // Resolve object type to find field index (struct field access)
                let field_idx = self.resolve_field_index(object, field);
                let dst = self.alloc_register();
                self.push_inst(Instruction::Load(dst, obj_reg, field_idx));
                dst
            }

            Expr::Assign { target, value } => {
                let val_reg = self.lower_expr(value);
                if let Expr::Ident(name) = &target.node {
                    // Release old value if reassigning a struct-typed variable
                    if let Some(&type_id) = self.local_types.get(name) {
                        if self.types.is_struct(type_id) {
                            if let Some(&old_reg) = self.locals.get(name) {
                                self.push_inst(Instruction::Release(old_reg, Some(type_id)));
                            }
                        }
                    }
                    self.locals.update(name, val_reg);
                } else if let Expr::FieldAccess { object, field } = &target.node {
                    let obj_reg = self.lower_expr(object);
                    let field_idx = self.resolve_field_index(object, field);
                    self.push_inst(Instruction::Store(obj_reg, field_idx, val_reg));
                } else if let Expr::IndexAccess { object, index } = &target.node {
                    let obj_reg = self.lower_expr(object);
                    let idx_reg = self.lower_expr(index);
                    self.push_inst(Instruction::IndexStore(obj_reg, idx_reg, val_reg));
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::CompoundAssign { op, target, value } => {
                // target = target op value
                let target_reg = self.lower_expr(target);
                let val_reg = self.lower_expr(value);
                let result_reg = self.alloc_register();
                let inst = match op {
                    BinOp::Add => Instruction::Add(result_reg, target_reg, val_reg),
                    BinOp::Sub => Instruction::Sub(result_reg, target_reg, val_reg),
                    BinOp::Mul => Instruction::Mul(result_reg, target_reg, val_reg),
                    BinOp::Div => Instruction::Div(result_reg, target_reg, val_reg),
                    BinOp::Mod => Instruction::Mod(result_reg, target_reg, val_reg),
                    BinOp::Shl => Instruction::Shl(result_reg, target_reg, val_reg),
                    BinOp::Shr => Instruction::Shr(result_reg, target_reg, val_reg),
                    BinOp::BitAnd => Instruction::BitAnd(result_reg, target_reg, val_reg),
                    BinOp::BitOr => Instruction::BitOr(result_reg, target_reg, val_reg),
                    BinOp::BitXor => Instruction::BitXor(result_reg, target_reg, val_reg),
                    _ => unreachable!(),
                };
                self.push_inst(inst);
                if let Expr::Ident(name) = &target.node {
                    self.locals.update(name, result_reg);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_reg = self.lower_expr(condition);

                let then_block = self.new_block_id();
                let else_block = self.new_block_id();
                let merge_block = self.new_block_id();

                // Pre-allocate result register
                let result_reg = self.alloc_register();

                // Snapshot locals before branching so we can reconcile mutations
                let pre_if_locals = self.locals.flatten();

                // End current block with branch
                self.finish_block(Terminator::Branch(cond_reg, then_block, else_block));

                // Then block
                self.start_block(then_block);
                let then_result = self.lower_block_expr(&then_branch.node);
                self.push_inst(Instruction::Copy(result_reg, then_result));
                self.finish_block(Terminator::Jump(merge_block));
                let then_block_idx = self.blocks.len() - 1;
                let post_then_locals = self.locals.flatten();

                // Restore locals to pre-if state before lowering else branch
                for (name, reg) in &pre_if_locals {
                    self.locals.update(name, *reg);
                }

                // Else block
                self.start_block(else_block);
                if let Some(else_expr) = else_branch {
                    let else_result = self.lower_expr(else_expr);
                    self.push_inst(Instruction::Copy(result_reg, else_result));
                } else {
                    self.push_inst(Instruction::ConstUnit(result_reg));
                }
                self.finish_block(Terminator::Jump(merge_block));
                let else_block_idx = self.blocks.len() - 1;
                let post_else_locals = self.locals.flatten();

                // Reconcile: for any variable mutated differently in the two
                // branches, emit Copy instructions into both branch blocks so
                // they agree on a single merge register.
                for (name, pre_reg) in &pre_if_locals {
                    let then_reg = post_then_locals.get(name).copied().unwrap_or(*pre_reg);
                    let else_reg = post_else_locals.get(name).copied().unwrap_or(*pre_reg);

                    if then_reg != else_reg {
                        // Both branches have different registers — merge them
                        let merge_reg = self.alloc_register();
                        self.blocks[then_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, then_reg));
                        self.blocks[then_block_idx]
                            .spans
                            .push(self.current_span);
                        self.blocks[else_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, else_reg));
                        self.blocks[else_block_idx]
                            .spans
                            .push(self.current_span);
                        self.locals.update(name, merge_reg);
                    } else if then_reg != *pre_reg {
                        // Both branches mutated to the same register
                        self.locals.update(name, then_reg);
                    }
                }

                // Merge block
                self.start_block(merge_block);
                result_reg
            }

            Expr::Cast { expr, target_type } => {
                let src = self.lower_expr(expr);
                let target_id = self.resolve_type_expr(&target_type.node);
                let dst = self.alloc_typed_register(target_id);
                self.push_inst(Instruction::Cast(dst, src, target_id));
                dst
            }

            Expr::While { condition, body, label } => {
                let cond_block = self.new_block_id();
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to condition block
                self.finish_block(Terminator::Jump(cond_block));

                // Snapshot locals before condition (these are the registers the condition uses)
                self.start_block(cond_block);
                let pre_loop_locals = self.locals.clone();
                let cond_reg = self.lower_expr(condition);
                self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

                // Body block
                self.start_block(body_block);
                self.loop_stack.push(LoopContext {
                    label: label.clone(),
                    continue_block: cond_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back: emit Copy instructions for any locals whose register changed
                // so that the condition block sees updated values on next iteration
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(cond_block));

                // Exit block — restore locals to pre-loop state
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Loop { body, label } => {
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to body block
                self.finish_block(Terminator::Jump(body_block));

                // Body block
                self.start_block(body_block);
                let pre_loop_locals = self.locals.clone();
                self.loop_stack.push(LoopContext {
                    label: label.clone(),
                    continue_block: body_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back for loop variables
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(body_block));

                // Exit block (reached by break) — restore locals
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Break { label, .. } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack.iter().rev().find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack.last().map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                };
                if let Some((break_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(break_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Continue { label } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack.iter().rev().find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack.last().map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                };
                if let Some((continue_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(continue_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Grouped(inner) => self.lower_expr(inner),

            Expr::Block(block) => self.lower_block_expr(block),

            Expr::TupleLiteral(elements) => {
                let elem_regs: Vec<Register> = elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .map(|e| self.infer_expr_type(e).unwrap_or(self.types.i32()))
                    .collect();
                let type_id = self.types.intern(nudl_core::types::TypeKind::Tuple(elem_types));
                let dst = self.alloc_register();
                self.push_inst(Instruction::TupleAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayLiteral(elements) => {
                let elem_regs: Vec<Register> = elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_type = if !elements.is_empty() {
                    self.infer_expr_type(&elements[0]).unwrap_or(self.types.i32())
                } else {
                    self.types.i32()
                };
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: elements.len(),
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayRepeat { value, count } => {
                let val_reg = self.lower_expr(value);
                let elem_type = self.infer_expr_type(value).unwrap_or(self.types.i32());
                let elem_regs: Vec<Register> = (0..*count).map(|_| val_reg).collect();
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: *count,
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::IndexAccess { object, index } => {
                let obj_reg = self.lower_expr(object);
                let idx_reg = self.lower_expr(index);
                let elem_type = self.infer_index_element_type(object);
                let dst = self.alloc_register();
                self.push_inst(Instruction::IndexLoad(dst, obj_reg, idx_reg, elem_type));
                dst
            }

            Expr::Range { .. } => {
                // Ranges are only used in for-loops; standalone range expressions
                // produce unit for now
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::For { binding, iter, body } => {
                // Desugar for-in to while loop
                match &iter.node {
                    Expr::Range { start, end, inclusive } => {
                        self.lower_for_range(binding, start, end, *inclusive, &body.node)
                    }
                    _ => {
                        // Array iteration
                        self.lower_for_array(binding, iter, &body.node)
                    }
                }
            }

            _ => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    /// Emit Copy instructions at the end of a loop body to propagate updated
    /// local variables back to the registers that the loop header references.
    fn emit_loop_copyback(&mut self, pre_loop_locals: &ScopedLocals<Register>) {
        let pre_flat = pre_loop_locals.flatten();
        for (name, pre_reg) in &pre_flat {
            if let Some(&current_reg) = self.locals.get(name) {
                if current_reg != *pre_reg {
                    self.push_inst(Instruction::Copy(*pre_reg, current_reg));
                    // Reset locals to use the original register so the condition
                    // block's hardcoded references remain valid
                    self.locals.update(name, *pre_reg);
                }
            }
        }
    }

    fn lower_short_circuit_and(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();
        let false_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, rhs_block, false_block));

        // If lhs is true, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, short-circuit
        self.start_block(false_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(false)));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    fn lower_short_circuit_or(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let true_block = self.new_block_id();
        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, true_block, rhs_block));

        // If lhs is true, short-circuit
        self.start_block(true_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(true)));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    /// Resolve the field index for a field access expression.
    fn resolve_field_index(
        &mut self,
        object: &nudl_core::span::Spanned<Expr>,
        field: &str,
    ) -> u32 {
        // Walk the object expression to find its type
        let type_id = self.infer_expr_type(object);
        if let Some(tid) = type_id {
            if let nudl_core::types::TypeKind::Struct { fields, .. } = self.types.resolve(tid) {
                if let Some(idx) = fields.iter().position(|(n, _)| n == field) {
                    return idx as u32;
                }
            }
        }
        0 // fallback (should have been caught by checker)
    }

    /// Resolve a TypeExpr to a TypeId (mirrors checker.rs resolve_type).
    fn resolve_type_expr(&mut self, ty: &TypeExpr) -> nudl_core::types::TypeId {
        match ty {
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
                _ => {
                    if let Some(&tid) = self.struct_defs.get(name.as_str()) {
                        tid
                    } else {
                        self.types.i64() // fallback
                    }
                }
            },
            TypeExpr::Tuple(elements) => {
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .map(|e| self.resolve_type_expr(&e.node))
                    .collect();
                self.types.intern(nudl_core::types::TypeKind::Tuple(elem_types))
            }
            TypeExpr::FixedArray { element, length } => {
                let elem_ty = self.resolve_type_expr(&element.node);
                self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_ty,
                    length: *length,
                })
            }
        }
    }

    /// Best-effort type inference for an expression (used in lowerer for field lookups).
    fn infer_expr_type(
        &mut self,
        expr: &nudl_core::span::Spanned<Expr>,
    ) -> Option<nudl_core::types::TypeId> {
        match &expr.node {
            Expr::Ident(name) => self.local_types.get(name).copied(),
            Expr::StructLiteral { name, .. } => {
                self.struct_defs.get(name.as_str()).copied()
            }
            Expr::FieldAccess { object, field } => {
                let obj_type = self.infer_expr_type(object)?;
                match self.types.resolve(obj_type) {
                    nudl_core::types::TypeKind::Struct { fields, .. } => {
                        fields.iter().find(|(n, _)| n == field).map(|(_, ty)| *ty)
                    }
                    nudl_core::types::TypeKind::Tuple(elements) => {
                        field.parse::<usize>().ok().and_then(|idx| elements.get(idx).copied())
                    }
                    _ => None,
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = &callee.node {
                    self.function_sigs.get(name).map(|sig| sig.return_type)
                } else {
                    None
                }
            }
            Expr::MethodCall { object, method, .. } => {
                let type_name = self.infer_receiver_type_name(object)?;
                let mangled = format!("{}__{}", type_name, method);
                self.function_sigs.get(&mangled).map(|sig| sig.return_type)
            }
            Expr::StaticCall { type_name, method, .. } => {
                let mangled = format!("{}__{}", type_name, method);
                self.function_sigs.get(&mangled).map(|sig| sig.return_type)
            }
            Expr::TupleLiteral(elements) => {
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .filter_map(|e| self.infer_expr_type(e))
                    .collect();
                if elem_types.len() == elements.len() {
                    Some(self.types.intern(nudl_core::types::TypeKind::Tuple(elem_types)))
                } else {
                    None
                }
            }
            Expr::ArrayLiteral(elements) => {
                if elements.is_empty() {
                    return None;
                }
                let elem_type = self.infer_expr_type(&elements[0])?;
                Some(self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: elements.len(),
                }))
            }
            Expr::ArrayRepeat { value, count } => {
                let elem_type = self.infer_expr_type(value)?;
                Some(self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: *count,
                }))
            }
            Expr::IndexAccess { object, .. } => {
                let obj_type = self.infer_expr_type(object)?;
                match self.types.resolve(obj_type) {
                    nudl_core::types::TypeKind::FixedArray { element, .. } => Some(*element),
                    _ => None,
                }
            }
            Expr::Literal(Literal::Int(_, Some(suffix))) => {
                Some(match suffix {
                    IntSuffix::I8 => self.types.i8(),
                    IntSuffix::I16 => self.types.i16(),
                    IntSuffix::I32 => self.types.i32(),
                    IntSuffix::I64 => self.types.i64(),
                    IntSuffix::U8 => self.types.u8(),
                    IntSuffix::U16 => self.types.u16(),
                    IntSuffix::U32 => self.types.u32(),
                    IntSuffix::U64 => self.types.u64(),
                })
            }
            Expr::Literal(Literal::Int(_, None)) => Some(self.types.i32()),
            Expr::Literal(Literal::Float(_)) => Some(self.types.f64()),
            Expr::Literal(Literal::Bool(_)) => Some(self.types.bool()),
            Expr::Literal(Literal::Char(_)) => Some(self.types.char_type()),
            Expr::Literal(Literal::String(_)) => Some(self.types.string()),
            _ => None,
        }
    }

    /// Infer element type for index access operations.
    fn infer_index_element_type(
        &mut self,
        object: &nudl_core::span::Spanned<Expr>,
    ) -> nudl_core::types::TypeId {
        if let Some(obj_type) = self.infer_expr_type(object) {
            match self.types.resolve(obj_type) {
                nudl_core::types::TypeKind::FixedArray { element, .. } => return *element,
                _ => {}
            }
        }
        self.types.i64() // fallback
    }

    /// Desugar `for i in start..end { body }` to a while loop
    fn lower_for_range(
        &mut self,
        binding: &str,
        start: &nudl_core::span::Spanned<Expr>,
        end: &nudl_core::span::Spanned<Expr>,
        inclusive: bool,
        body: &Block,
    ) -> Register {
        // let mut __iter = start;
        let iter_reg = self.lower_expr(start);
        let end_reg = self.lower_expr(end);

        let iter_name = format!("__for_iter_{}", binding);
        self.locals.insert(iter_name.clone(), iter_reg);

        let cond_block = self.new_block_id();
        let body_block = self.new_block_id();
        let incr_block = self.new_block_id();
        let exit_block = self.new_block_id();

        // Jump to condition
        self.finish_block(Terminator::Jump(cond_block));

        // Condition: __iter < end (or __iter <= end for inclusive)
        self.start_block(cond_block);
        let pre_loop_locals = self.locals.clone();
        let cur_iter = self.locals.get(&iter_name).copied().unwrap();
        let cond_reg = self.alloc_register();
        if inclusive {
            self.push_inst(Instruction::Le(cond_reg, cur_iter, end_reg));
        } else {
            self.push_inst(Instruction::Lt(cond_reg, cur_iter, end_reg));
        }
        self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

        // Body block
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: incr_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // let binding = __iter;
        let binding_reg = self.locals.get(&iter_name).copied().unwrap();
        self.locals.insert(binding.to_string(), binding_reg);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Fall through to increment block
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(incr_block));

        // Increment block: __iter = __iter + 1, then jump to cond
        self.start_block(incr_block);
        let cur_iter = self.locals.get(&iter_name).copied().unwrap();
        let one_reg = self.alloc_register();
        self.push_inst(Instruction::Const(one_reg, ConstValue::I32(1)));
        let next_iter = self.alloc_register();
        self.push_inst(Instruction::Add(next_iter, cur_iter, one_reg));
        self.locals.update(&iter_name, next_iter);

        // Copy-back loop variables
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(cond_block));

        // Exit block
        self.start_block(exit_block);
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }

    /// Desugar `for item in array { body }` to a while loop with indexing
    fn lower_for_array(
        &mut self,
        binding: &str,
        iter_expr: &nudl_core::span::Spanned<Expr>,
        body: &Block,
    ) -> Register {
        let arr_reg = self.lower_expr(iter_expr);
        let arr_type = self.infer_expr_type(iter_expr);

        let length = if let Some(tid) = arr_type {
            match self.types.resolve(tid) {
                nudl_core::types::TypeKind::FixedArray { length, .. } => *length,
                _ => 0,
            }
        } else {
            0
        };

        let elem_type = if let Some(tid) = arr_type {
            match self.types.resolve(tid) {
                nudl_core::types::TypeKind::FixedArray { element, .. } => *element,
                _ => self.types.i64(),
            }
        } else {
            self.types.i64()
        };

        // let mut __idx = 0;
        let idx_reg = self.alloc_register();
        self.push_inst(Instruction::Const(idx_reg, ConstValue::I32(0)));
        let idx_name = format!("__for_idx_{}", binding);
        self.locals.insert(idx_name.clone(), idx_reg);

        let len_reg = self.alloc_register();
        self.push_inst(Instruction::Const(len_reg, ConstValue::I32(length as i32)));

        let cond_block = self.new_block_id();
        let body_block = self.new_block_id();
        let incr_block = self.new_block_id();
        let exit_block = self.new_block_id();

        self.finish_block(Terminator::Jump(cond_block));

        // Condition: __idx < len
        self.start_block(cond_block);
        let pre_loop_locals = self.locals.clone();
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let cond_reg = self.alloc_register();
        self.push_inst(Instruction::Lt(cond_reg, cur_idx, len_reg));
        self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

        // Body block
        self.start_block(body_block);
        self.loop_stack.push(LoopContext {
            label: None,
            continue_block: incr_block,
            break_block: exit_block,
            pre_loop_locals: pre_loop_locals.clone(),
        });

        // let binding = arr[__idx];
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let elem_reg = self.alloc_register();
        self.push_inst(Instruction::IndexLoad(elem_reg, arr_reg, cur_idx, elem_type));
        self.locals.insert(binding.to_string(), elem_reg);

        self.lower_block_expr(body);

        self.loop_stack.pop();

        // Fall through to increment block
        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(incr_block));

        // Increment block: __idx = __idx + 1, then jump to cond
        self.start_block(incr_block);
        let cur_idx = self.locals.get(&idx_name).copied().unwrap();
        let one_reg = self.alloc_register();
        self.push_inst(Instruction::Const(one_reg, ConstValue::I32(1)));
        let next_idx = self.alloc_register();
        self.push_inst(Instruction::Add(next_idx, cur_idx, one_reg));
        self.locals.update(&idx_name, next_idx);

        self.emit_loop_copyback(&pre_loop_locals);
        self.finish_block(Terminator::Jump(cond_block));

        // Exit block
        self.start_block(exit_block);
        self.locals = pre_loop_locals;
        let unit_reg = self.alloc_register();
        self.push_inst(Instruction::ConstUnit(unit_reg));
        unit_reg
    }

    fn lower_builtin_call(&mut self, name: &str, args: &[CallArg]) -> Register {
        let call_span = self.current_span;
        match name {
            "__str_ptr" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let dst = self.alloc_register();
                self.push_inst(Instruction::StringPtr(dst, arg_reg));
                dst
            }
            "__str_len" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let dst = self.alloc_register();
                self.push_inst(Instruction::StringLen(dst, arg_reg));
                dst
            }
            _ => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    fn lower_generic_call(&mut self, name: &str, args: &[CallArg], is_extern: bool) -> Register {
        let call_span = self.current_span;
        // Lower all arguments
        let arg_regs: Vec<Register> = args.iter().map(|arg| self.lower_expr(&arg.value)).collect();
        self.current_span = call_span;

        // Caller-retain: for struct-typed args, emit Retain so callee's Release doesn't free them
        if !is_extern {
            if let Some(sig) = self.function_sigs.get(name).cloned() {
                for (i, (_pname, pty)) in sig.params.iter().enumerate() {
                    if self.types.is_struct(*pty) && i < arg_regs.len() {
                        self.push_inst(Instruction::Retain(arg_regs[i]));
                    }
                }
            }
        }

        let sym = self.interner.intern(name);

        let func_ref = if is_extern {
            FunctionRef::Extern(sym)
        } else {
            FunctionRef::Named(sym)
        };

        let ret_type = self.function_sigs.get(name).map(|s| s.return_type).unwrap_or(self.types.i64());
        let dst = self.alloc_typed_register(ret_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }

    /// Resolve call arguments to positional registers, handling named args and defaults.
    /// `skip_params` is the number of leading params to skip for argument matching (e.g., self).
    /// `fn_key` is the key used to look up default values in `param_defaults`.
    fn resolve_call_args(
        &mut self,
        fn_key: &str,
        sig: &FunctionSig,
        call_args: &[CallArg],
        skip_params: usize,
    ) -> Vec<Register> {
        let call_span = self.current_span;
        let callable_params = &sig.params[skip_params..];
        let callable_defaults = &sig.has_default[skip_params..];

        // Build array of resolved registers, one per callable param
        let mut resolved: Vec<Option<Register>> = vec![None; callable_params.len()];

        // Process positional args first
        let mut positional_idx = 0;
        for arg in call_args {
            if arg.name.is_some() {
                break;
            }
            if positional_idx < callable_params.len() {
                let reg = self.lower_expr(&arg.value);
                resolved[positional_idx] = Some(reg);
                positional_idx += 1;
            }
        }

        // Process named args
        for arg in call_args.iter().skip(positional_idx) {
            if let Some(arg_name) = &arg.name {
                if let Some(pos) = callable_params.iter().position(|(n, _)| n == arg_name) {
                    let reg = self.lower_expr(&arg.value);
                    resolved[pos] = Some(reg);
                }
            } else {
                // Positional arg that came after named
                if positional_idx < callable_params.len() {
                    let reg = self.lower_expr(&arg.value);
                    resolved[positional_idx] = Some(reg);
                    positional_idx += 1;
                }
            }
        }

        self.current_span = call_span;

        // Fill in defaults for missing args
        let defaults = self.param_defaults.get(fn_key).cloned();

        for (i, resolved_reg) in resolved.iter_mut().enumerate() {
            if resolved_reg.is_none() && callable_defaults[i] {
                // Try to lower the default expression from the AST
                let param_idx = i + skip_params;
                let did_lower = if let Some(ref defaults_vec) = defaults {
                    if let Some(Some(default_expr)) = defaults_vec.get(param_idx) {
                        let reg = self.lower_expr(default_expr);
                        *resolved_reg = Some(reg);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !did_lower {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    *resolved_reg = Some(reg);
                }
            }
        }

        resolved.into_iter().map(|r| r.unwrap_or_else(|| {
            let reg = self.alloc_register();
            self.push_inst(Instruction::ConstUnit(reg));
            reg
        })).collect()
    }

    /// Lower a function call with named arg resolution and default filling.
    fn lower_resolved_call(
        &mut self,
        name: &str,
        sig: &FunctionSig,
        args: &[CallArg],
        is_extern: bool,
        skip_params: usize,
    ) -> Register {
        // For extern or simple calls without named args and with all args provided, fast path
        let has_named = args.iter().any(|a| a.name.is_some());
        let all_provided = args.len() + skip_params == sig.params.len();

        if !has_named && all_provided {
            return self.lower_generic_call(name, args, is_extern);
        }

        let call_span = self.current_span;
        let arg_regs = self.resolve_call_args(name, sig, args, skip_params);
        self.current_span = call_span;

        // Caller-retain for struct-typed args
        if !is_extern {
            for (i, (_pname, pty)) in sig.params[skip_params..].iter().enumerate() {
                if self.types.is_struct(*pty) && i < arg_regs.len() {
                    self.push_inst(Instruction::Retain(arg_regs[i]));
                }
            }
        }

        let sym = self.interner.intern(name);
        let func_ref = if is_extern {
            FunctionRef::Extern(sym)
        } else {
            FunctionRef::Named(sym)
        };

        let dst = self.alloc_typed_register(sig.return_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }

    /// Lower a method call: self is already lowered, pass it as first arg
    fn lower_method_call(
        &mut self,
        mangled_name: &str,
        sig: &FunctionSig,
        self_reg: Register,
        args: &[CallArg],
    ) -> Register {
        let call_span = self.current_span;

        // Resolve the rest of the args (skip self)
        let mut arg_regs = vec![self_reg];
        let rest = self.resolve_call_args(mangled_name, sig, args, 1);
        arg_regs.extend(rest);

        self.current_span = call_span;

        // Caller-retain for struct-typed args
        for (i, (_pname, pty)) in sig.params.iter().enumerate() {
            if self.types.is_struct(*pty) && i < arg_regs.len() {
                self.push_inst(Instruction::Retain(arg_regs[i]));
            }
        }

        let sym = self.interner.intern(mangled_name);
        let func_ref = FunctionRef::Named(sym);

        let dst = self.alloc_typed_register(sig.return_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }

    /// Infer the type name of the receiver expression for method resolution.
    fn infer_receiver_type_name(&self, expr: &nudl_core::span::Spanned<Expr>) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => {
                if let Some(ty_id) = self.local_types.get(name) {
                    if let TypeKind::Struct { name, .. } = self.types.resolve(*ty_id) {
                        return Some(name.clone());
                    }
                }
                None
            }
            Expr::StructLiteral { name, .. } => Some(name.clone()),
            Expr::StaticCall { type_name, method, .. } => {
                let mangled = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled) {
                    if let TypeKind::Struct { name, .. } = self.types.resolve(sig.return_type) {
                        return Some(name.clone());
                    }
                }
                None
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(fn_name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(fn_name.as_str()) {
                        if let TypeKind::Struct { name, .. } = self.types.resolve(sig.return_type) {
                            return Some(name.clone());
                        }
                    }
                }
                None
            }
            Expr::MethodCall { object, method, .. } => {
                // Recurse: infer receiver type, then look up method return type
                if let Some(type_name) = self.infer_receiver_type_name(object) {
                    let mangled = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled) {
                        if let TypeKind::Struct { name, .. } = self.types.resolve(sig.return_type) {
                            return Some(name.clone());
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::Checker;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_core::span::FileId;

    fn lower_source(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        Lowerer::new(checked).lower(&module)
    }

    #[test]
    fn lower_target_program() {
        let program = lower_source(
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

        // 4 functions: write (extern), print, println, main
        assert_eq!(
            program.functions.len(),
            4,
            "expected 4 functions, got {}",
            program.functions.len()
        );

        // write should be extern
        let write_fn = &program.functions[0];
        assert!(write_fn.is_extern);
        assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));

        // String constants should include "Hello, world!" and "\n"
        assert!(
            program
                .string_constants
                .contains(&"Hello, world!".to_string()),
            "missing 'Hello, world!' in {:?}",
            program.string_constants
        );
        assert!(
            program.string_constants.contains(&"\n".to_string()),
            "missing '\\n' in {:?}",
            program.string_constants
        );

        // Entry function should be main
        assert!(program.entry_function.is_some());

        // print function should have StringPtr and StringLen instructions
        let print_fn = &program.functions[1];
        assert!(!print_fn.is_extern);
        assert_eq!(print_fn.params.len(), 1);
        let has_str_ptr = print_fn.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::StringPtr(_, _)))
        });
        let has_str_len = print_fn.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::StringLen(_, _)))
        });
        assert!(has_str_ptr, "print should have StringPtr instruction");
        assert!(has_str_len, "print should have StringLen instruction");
    }

    #[test]
    fn lower_has_return() {
        let program = lower_source(
            r#"
fn main() {
    __str_ptr("hi");
}
"#,
        );
        let main_func = program.functions.iter().find(|f| !f.is_extern).unwrap();
        let last_block = main_func.blocks.last().unwrap();
        assert!(matches!(last_block.terminator, Terminator::Return(_)));
    }

    #[test]
    fn extern_function_lowered() {
        let program = lower_source(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn main() {}
"#,
        );
        let write_fn = program.functions.iter().find(|f| f.is_extern).unwrap();
        assert!(write_fn.blocks.is_empty());
        assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));
    }

    #[test]
    fn params_assigned_to_registers() {
        let program = lower_source(
            r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#,
        );
        let greet_fn = &program.functions[0];
        assert_eq!(greet_fn.params.len(), 1);
    }

    #[test]
    fn string_dedup() {
        let program = lower_source(
            r#"
fn main() {
    __str_ptr("same");
    __str_ptr("same");
}
"#,
        );
        // "same" should appear only once
        assert_eq!(
            program
                .string_constants
                .iter()
                .filter(|s| *s == "same")
                .count(),
            1
        );
    }

    #[test]
    fn lower_binary_ops() {
        let program = lower_source(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    add(1, 2);
}
"#,
        );
        let add_fn = &program.functions[0];
        let has_add = add_fn.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::Add(_, _, _)))
        });
        assert!(has_add, "add function should have Add instruction");
    }

    #[test]
    fn lower_if_creates_blocks() {
        let program = lower_source(
            r#"
fn main() {
    let x: i32 = 10;
    if x > 5 {
        __str_ptr("yes");
    } else {
        __str_ptr("no");
    }
}
"#,
        );
        let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
        // If/else should create multiple blocks
        assert!(
            main_fn.blocks.len() >= 4,
            "expected at least 4 blocks for if/else, got {}",
            main_fn.blocks.len()
        );
    }

    #[test]
    fn lower_while_creates_blocks() {
        let program = lower_source(
            r#"
fn main() {
    let mut x: i32 = 0;
    while x < 10 {
        x = x + 1;
    }
}
"#,
        );
        let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
        // While should create multiple blocks
        assert!(
            main_fn.blocks.len() >= 3,
            "expected at least 3 blocks for while, got {}",
            main_fn.blocks.len()
        );
    }

    #[test]
    fn lower_target_program_v2() {
        let program = lower_source(
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
        assert!(program.entry_function.is_some());
        assert!(
            program.functions.len() >= 5,
            "expected at least 5 functions (write, print, println, add, main)"
        );
    }

    #[test]
    fn lower_struct_alloc_store_load() {
        let program = lower_source(
            r#"
struct Point { x: i32, y: i32 }
fn main() {
    let p = Point { x: 42, y: 17 };
    let val = p.x;
}
"#,
        );
        let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
        let all_insts: Vec<&Instruction> = main_fn
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .collect();

        assert!(
            all_insts.iter().any(|i| matches!(i, Instruction::Alloc(_, _))),
            "expected Alloc instruction for struct literal"
        );
        assert!(
            all_insts.iter().any(|i| matches!(i, Instruction::Store(_, _, _))),
            "expected Store instruction for field init"
        );
        assert!(
            all_insts.iter().any(|i| matches!(i, Instruction::Load(_, _, _))),
            "expected Load instruction for field access"
        );
        assert!(
            all_insts.iter().any(|i| matches!(i, Instruction::Release(_, _))),
            "expected Release instruction for scope exit"
        );
    }

    #[test]
    fn lower_struct_caller_retain_callee_release() {
        let program = lower_source(
            r#"
struct Point { x: i32, y: i32 }
fn use_point(p: Point) {
    let val = p.x;
}
fn main() {
    let p = Point { x: 1, y: 2 };
    use_point(p);
}
"#,
        );
        // Check main has Retain (caller-retain before calling use_point)
        let main_fn = program
            .functions
            .iter()
            .find(|f| {
                let name = program.interner.resolve(f.name);
                name == "main"
            })
            .unwrap();
        let main_insts: Vec<&Instruction> = main_fn
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .collect();
        assert!(
            main_insts.iter().any(|i| matches!(i, Instruction::Retain(_))),
            "expected Retain in main (caller-retain)"
        );

        // Check use_point has Release (callee-release of param)
        let use_fn = program
            .functions
            .iter()
            .find(|f| {
                let name = program.interner.resolve(f.name);
                name == "use_point"
            })
            .unwrap();
        let use_insts: Vec<&Instruction> = use_fn
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .collect();
        assert!(
            use_insts.iter().any(|i| matches!(i, Instruction::Release(_, _))),
            "expected Release in use_point (callee-release)"
        );
    }

    // --- Phase 3: Named arguments ---

    #[test]
    fn named_arguments_basic() {
        let program = lower_source(
            r#"
fn power(base: i32, exponent: i32) -> i32 {
    base * exponent
}

fn main() {
    let r = power(2, exponent: 3);
}
"#,
        );
        assert!(program.entry_function.is_some());
    }

    #[test]
    fn named_arguments_all_named() {
        let program = lower_source(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let r = add(a: 1, b: 2);
}
"#,
        );
        assert!(program.entry_function.is_some());
    }

    // --- Phase 3: Default parameters ---

    #[test]
    fn default_params_basic() {
        let program = lower_source(
            r#"
fn repeat_string(s: string, times: i32 = 3) -> i32 {
    times
}

fn main() {
    let a = repeat_string("hello");
    let b = repeat_string("world", times: 5);
}
"#,
        );
        assert!(program.entry_function.is_some());
    }

    #[test]
    fn default_params_multiple() {
        let program = lower_source(
            r#"
fn connect(host: string, port: i32 = 8080, timeout_ms: i32 = 30000) -> i32 {
    port
}

fn main() {
    let a = connect("localhost");
    let b = connect("localhost", port: 9090);
    let c = connect("localhost", port: 9090, timeout_ms: 5000);
}
"#,
        );
        assert!(program.entry_function.is_some());
    }

    // --- Phase 3: Impl blocks and methods ---

    #[test]
    fn impl_block_static_method() {
        let program = lower_source(
            r#"
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Point {
        Point { x: x, y: y }
    }
}

fn main() {
    let p = Point::new(3, y: 4);
}
"#,
        );
        // Should have a function named Point__new
        let has_point_new = program.functions.iter().any(|f| {
            program.interner.resolve(f.name) == "Point__new"
        });
        assert!(has_point_new, "expected Point__new function");
    }

    #[test]
    fn impl_block_instance_method() {
        let program = lower_source(
            r#"
struct Counter {
    value: i32,
}

impl Counter {
    fn new(start: i32) -> Counter {
        Counter { value: start }
    }

    fn get(self) -> i32 {
        self.value
    }

    fn increment(mut self) {
        self.value = self.value + 1;
    }
}

fn main() {
    let mut c = Counter::new(0);
    c.increment();
    let v = c.get();
}
"#,
        );
        let has_counter_get = program.functions.iter().any(|f| {
            program.interner.resolve(f.name) == "Counter__get"
        });
        let has_counter_increment = program.functions.iter().any(|f| {
            program.interner.resolve(f.name) == "Counter__increment"
        });
        assert!(has_counter_get, "expected Counter__get function");
        assert!(has_counter_increment, "expected Counter__increment function");
    }

    // --- Phase 3: Struct field shorthand ---

    #[test]
    fn struct_field_shorthand() {
        let program = lower_source(
            r#"
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let x = 3;
    let y = 4;
    let p = Point { x, y };
}
"#,
        );
        assert!(program.entry_function.is_some());
    }
}
