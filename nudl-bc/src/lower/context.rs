use std::collections::HashMap;

use nudl_core::intern::StringInterner;
use nudl_core::span::Span;
use nudl_core::types::TypeInterner;

use nudl_ast::ast::*;

use crate::checker::FunctionSig;
use crate::ir::*;
use crate::scoped_locals::ScopedLocals;

use super::LoopContext;

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
}
