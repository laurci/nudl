use std::collections::HashMap;

use nudl_core::intern::StringInterner;
use nudl_core::span::Span;
use nudl_core::types::TypeInterner;

use nudl_ast::ast::*;

use crate::checker::FunctionSig;
use crate::ir::*;
use crate::scoped_locals::ScopedLocals;

use super::{LoopContext, PendingClosure};

pub struct FunctionLowerCtx<'a> {
    pub(super) blocks: Vec<BasicBlock>,
    pub(super) current_block_id: BlockId,
    pub(super) current_instructions: Vec<Instruction>,
    pub(super) current_spans: Vec<Span>,
    pub(super) current_span: Span,
    pub(super) next_block_id: u32,
    pub(super) next_register: u32,
    pub(super) locals: ScopedLocals<Register>,
    /// Track which locals are struct-typed (for Release at scope exit)
    pub(super) local_types: ScopedLocals<nudl_core::types::TypeId>,
    /// TypeId for each register, indexed by Register.0
    pub(super) register_types: Vec<nudl_core::types::TypeId>,
    pub(super) string_constants: &'a mut Vec<String>,
    pub(super) interner: &'a mut StringInterner,
    pub(super) function_sigs: &'a HashMap<String, FunctionSig>,
    pub(super) struct_defs: &'a HashMap<String, nudl_core::types::TypeId>,
    pub(super) enum_defs: &'a HashMap<String, nudl_core::types::TypeId>,
    pub(super) types: &'a mut TypeInterner,
    pub(super) loop_stack: Vec<LoopContext>,
    /// Default parameter expressions for all functions (for filling in defaults at call sites)
    pub(super) param_defaults: &'a HashMap<String, Vec<Option<SpannedExpr>>>,
    /// Deferred blocks to emit at scope exit (LIFO order)
    pub(super) deferred_blocks: Vec<Block>,
    /// Pending closures to be lowered as separate functions
    pub(super) pending_closures: &'a mut Vec<PendingClosure>,
    /// Next function ID counter (shared with Lowerer)
    pub(super) next_function_id: &'a mut u32,
    /// Return type of the current function (for ? operator early return)
    pub(super) return_type: nudl_core::types::TypeId,
    /// Hint for inferring closure parameter types from the expected function type.
    pub(super) closure_type_hint: Option<nudl_core::types::TypeId>,
    /// Name of the function currently being lowered (for call resolution context)
    pub(super) current_fn_name: String,
    /// Generic call site -> mangled function name (keyed by (current_fn_name, call_span))
    pub(super) call_resolutions: &'a HashMap<(String, Span), String>,
    /// Generic struct literal -> mangled struct name
    pub(super) struct_resolutions: &'a HashMap<Span, String>,
    /// Generic enum constructor -> mangled enum name
    pub(super) enum_resolutions: &'a HashMap<Span, String>,
    /// Accumulated warnings from type resolution fallbacks (internal bugs)
    pub(super) lowering_warnings: Vec<String>,
    /// Type parameter substitution map for monomorphized functions (e.g., "T" -> i32)
    pub(super) type_param_subst: HashMap<String, nudl_core::types::TypeId>,
    /// Interface method names for vtable building: interface_name -> list of method names
    pub(super) interface_methods: &'a HashMap<String, Vec<String>>,
    /// Map from interface name -> set of implementing type names
    pub(super) interface_impls: &'a HashMap<String, Vec<String>>,
    /// Dynamic dispatch call resolutions: span → (interface_name, method_index)
    pub(super) dyn_call_resolutions: &'a HashMap<Span, (String, usize)>,
    /// Vtable lookup: (concrete_type, interface_name) → vtable_index
    pub(super) vtable_lookup: &'a HashMap<(String, String), u32>,
}

impl<'a> FunctionLowerCtx<'a> {
    pub(super) fn alloc_register(&mut self) -> Register {
        let r = Register(self.next_register);
        self.next_register += 1;
        // Default to i64
        let default_ty = self.types.i64();
        self.register_types.push(default_ty);
        r
    }

    pub(super) fn alloc_typed_register(&mut self, type_id: nudl_core::types::TypeId) -> Register {
        let r = Register(self.next_register);
        self.next_register += 1;
        self.register_types.push(type_id);
        r
    }

    pub(super) fn new_block_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    /// Finish the current block with the given terminator and start a new block
    pub(super) fn finish_block(&mut self, terminator: Terminator) -> BlockId {
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
    pub(super) fn start_block(&mut self, id: BlockId) {
        self.current_block_id = id;
        self.current_instructions.clear();
        self.current_spans.clear();
    }

    /// Push an instruction along with the current span
    pub(super) fn push_inst(&mut self, inst: Instruction) {
        self.current_instructions.push(inst);
        self.current_spans.push(self.current_span);
    }

    /// Emit Release instructions for any type that requires ARC management.
    /// Handles reference types (excluding String at SSA level) and fixed arrays of reference-typed elements.
    pub(super) fn emit_release_for_type(
        &mut self,
        reg: Register,
        type_id: nudl_core::types::TypeId,
    ) {
        if self.types.is_reference_type(type_id)
            && !matches!(
                self.types.resolve(type_id),
                nudl_core::types::TypeKind::String
            )
        {
            self.push_inst(Instruction::Release(reg, Some(type_id)));
        } else if let nudl_core::types::TypeKind::FixedArray { element, length } =
            self.types.resolve(type_id)
        {
            let elem = *element;
            let len = *length;
            if self.types.is_reference_type(elem)
                && !matches!(self.types.resolve(elem), nudl_core::types::TypeKind::String)
            {
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

    /// Emit Retain instruction for reference types (excluding String at SSA level).
    pub(super) fn emit_retain_for_type(
        &mut self,
        reg: Register,
        type_id: nudl_core::types::TypeId,
    ) {
        if self.types.is_reference_type(type_id)
            && !matches!(
                self.types.resolve(type_id),
                nudl_core::types::TypeKind::String
            )
        {
            self.push_inst(Instruction::Retain(reg));
        }
    }

    /// Construct a mangled type name for generic type lookup (mirrors checker's mangle_name).
    pub(super) fn mangle_type_name(
        &self,
        base: &str,
        type_args: &[nudl_core::types::TypeId],
    ) -> String {
        let mut name = base.to_string();
        for &ty in type_args {
            name.push('$');
            name.push_str(&self.type_name_for_mangle(ty));
        }
        name
    }

    /// Convert a TypeId to its string name for mangling (mirrors checker's type_name).
    fn type_name_for_mangle(&self, ty: nudl_core::types::TypeId) -> String {
        use nudl_core::types::{PrimitiveType, TypeKind};
        match self.types.resolve(ty) {
            TypeKind::Primitive(p) => match p {
                PrimitiveType::Char => "char".into(),
                p => format!("{:?}", p).to_lowercase(),
            },
            TypeKind::String => "string".into(),
            TypeKind::Struct { name, .. } | TypeKind::Enum { name, .. } => name.clone(),
            TypeKind::Tuple(elements) => {
                let parts: Vec<String> = elements
                    .iter()
                    .map(|e| self.type_name_for_mangle(*e))
                    .collect();
                format!("({})", parts.join(", "))
            }
            TypeKind::FixedArray { element, length } => {
                format!("[{}; {}]", self.type_name_for_mangle(*element), length)
            }
            TypeKind::DynamicArray { element } => {
                format!("{}[]", self.type_name_for_mangle(*element))
            }
            TypeKind::Map { key, value } => {
                format!(
                    "Map<{}, {}>",
                    self.type_name_for_mangle(*key),
                    self.type_name_for_mangle(*value)
                )
            }
            TypeKind::Function { params, ret } => {
                let param_strs: Vec<String> = params
                    .iter()
                    .map(|p| self.type_name_for_mangle(*p))
                    .collect();
                let is_unit = matches!(
                    self.types.resolve(*ret),
                    TypeKind::Primitive(PrimitiveType::Unit)
                );
                if is_unit {
                    format!("|{}|", param_strs.join(", "))
                } else {
                    format!(
                        "|{}| -> {}",
                        param_strs.join(", "),
                        self.type_name_for_mangle(*ret)
                    )
                }
            }
            _ => "i64".into(),
        }
    }

    /// Allocate a new function ID for a closure thunk
    pub(super) fn alloc_closure_function_id(&mut self) -> FunctionId {
        let id = FunctionId(*self.next_function_id);
        *self.next_function_id += 1;
        id
    }

    /// Collect free variables referenced in an expression that are in the current scope.
    /// Returns (name, register, type) for each captured variable.
    pub(super) fn collect_captures(
        &self,
        expr: &nudl_core::span::Spanned<Expr>,
        param_names: &[String],
    ) -> Vec<(String, Register, nudl_core::types::TypeId)> {
        let mut captures = Vec::new();
        let mut seen = std::collections::HashSet::new();
        self.collect_captures_inner(&expr.node, param_names, &mut captures, &mut seen);
        captures
    }

    fn collect_captures_inner(
        &self,
        expr: &Expr,
        param_names: &[String],
        captures: &mut Vec<(String, Register, nudl_core::types::TypeId)>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        match expr {
            Expr::Ident(name) => {
                // If this name is in scope as a local (not a closure param) and not yet captured
                if !param_names.contains(name) && !seen.contains(name) {
                    if let Some(reg) = self.locals.get(name) {
                        if let Some(ty) = self.local_types.get(name) {
                            seen.insert(name.clone());
                            captures.push((name.clone(), *reg, *ty));
                        }
                    }
                }
            }
            Expr::Block(block) => {
                for stmt in &block.stmts {
                    self.collect_captures_stmt(&stmt.node, param_names, captures, seen);
                }
                if let Some(tail) = &block.tail_expr {
                    self.collect_captures_inner(&tail.node, param_names, captures, seen);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.collect_captures_inner(&left.node, param_names, captures, seen);
                self.collect_captures_inner(&right.node, param_names, captures, seen);
            }
            Expr::Assign { target, value } | Expr::CompoundAssign { target, value, .. } => {
                self.collect_captures_inner(&target.node, param_names, captures, seen);
                self.collect_captures_inner(&value.node, param_names, captures, seen);
            }
            Expr::Unary { operand, .. } => {
                self.collect_captures_inner(&operand.node, param_names, captures, seen);
            }
            Expr::Call { callee, args, .. } => {
                self.collect_captures_inner(&callee.node, param_names, captures, seen);
                for arg in args {
                    self.collect_captures_inner(&arg.value.node, param_names, captures, seen);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                self.collect_captures_inner(&object.node, param_names, captures, seen);
                for arg in args {
                    self.collect_captures_inner(&arg.value.node, param_names, captures, seen);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_captures_inner(&object.node, param_names, captures, seen);
            }
            Expr::IndexAccess { object, index } => {
                self.collect_captures_inner(&object.node, param_names, captures, seen);
                self.collect_captures_inner(&index.node, param_names, captures, seen);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_captures_inner(&condition.node, param_names, captures, seen);
                self.collect_captures_inner(
                    &Expr::Block(then_branch.node.clone()),
                    param_names,
                    captures,
                    seen,
                );
                if let Some(eb) = else_branch {
                    self.collect_captures_inner(&eb.node, param_names, captures, seen);
                }
            }
            Expr::Return(Some(inner)) => {
                self.collect_captures_inner(&inner.node, param_names, captures, seen);
            }
            Expr::Grouped(inner) => {
                self.collect_captures_inner(&inner.node, param_names, captures, seen);
            }
            Expr::TupleLiteral(elements) | Expr::ArrayLiteral(elements) => {
                for e in elements {
                    self.collect_captures_inner(&e.node, param_names, captures, seen);
                }
            }
            Expr::Literal(Literal::TemplateString { exprs, .. }) => {
                for e in exprs {
                    self.collect_captures_inner(&e.node, param_names, captures, seen);
                }
            }
            Expr::Closure { body, params, .. } => {
                // For nested closures, don't descend into the body with our param list;
                // the nested closure's params shadow
                let mut nested_params: Vec<String> = param_names.to_vec();
                for p in params {
                    nested_params.push(p.name.clone());
                }
                self.collect_captures_inner(&body.node, &nested_params, captures, seen);
            }
            Expr::Cast { expr: inner, .. } => {
                self.collect_captures_inner(&inner.node, param_names, captures, seen);
            }
            _ => {}
        }
    }

    fn collect_captures_stmt(
        &self,
        stmt: &Stmt,
        param_names: &[String],
        captures: &mut Vec<(String, Register, nudl_core::types::TypeId)>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        match stmt {
            Stmt::Let { value, .. } => {
                self.collect_captures_inner(&value.node, param_names, captures, seen);
            }
            Stmt::Expr(expr) => {
                self.collect_captures_inner(&expr.node, param_names, captures, seen);
            }
            Stmt::Const { value, .. } => {
                self.collect_captures_inner(&value.node, param_names, captures, seen);
            }
            Stmt::LetPattern { value, .. } => {
                self.collect_captures_inner(&value.node, param_names, captures, seen);
            }
            _ => {}
        }
    }
}
