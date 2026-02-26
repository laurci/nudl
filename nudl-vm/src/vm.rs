use std::collections::HashMap;

use nudl_bc::ir::*;
use nudl_core::intern::Symbol;

use crate::helpers::is_truthy;
use crate::types::HeapObject;
use crate::{Value, VmError};

pub(crate) const DEFAULT_STEP_LIMIT: u64 = 1_000_000;
pub(crate) const MAX_CALL_DEPTH: usize = 256;

/// Register-based SSA bytecode interpreter.
pub struct Vm {
    pub(crate) step_count: u64,
    pub(crate) step_limit: u64,
    pub(crate) call_depth: usize,
    /// Simulated heap for ARC objects (comptime).
    pub(crate) heap: HashMap<u64, HeapObject>,
    pub(crate) next_heap_id: u64,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            step_count: 0,
            step_limit: DEFAULT_STEP_LIMIT,
            call_depth: 0,
            heap: HashMap::new(),
            next_heap_id: 1, // 0 reserved for "null"
        }
    }

    pub fn with_step_limit(limit: u64) -> Self {
        Self {
            step_count: 0,
            step_limit: limit,
            call_depth: 0,
            heap: HashMap::new(),
            next_heap_id: 1,
        }
    }

    /// Run the program starting from the entry function.
    pub fn run(&mut self, program: &Program) -> Result<Value, VmError> {
        let entry_id = program.entry_function.ok_or(VmError::NoEntryFunction)?;

        // Build function lookup: Symbol -> index in program.functions
        let func_map: HashMap<Symbol, usize> = program
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name, i))
            .collect();

        let entry_idx = program
            .functions
            .iter()
            .position(|f| f.id == entry_id)
            .ok_or(VmError::NoEntryFunction)?;

        self.execute_function(program, &func_map, entry_idx, vec![])
    }

    pub(crate) fn execute_function(
        &mut self,
        program: &Program,
        func_map: &HashMap<Symbol, usize>,
        func_idx: usize,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let func = &program.functions[func_idx];
        let func_name = program.interner.resolve(func.name).to_string();

        // Check for extern function
        if func.is_extern {
            return Err(VmError::ExternCallNotAllowed {
                function_name: func_name,
            });
        }

        // Check call depth
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(VmError::StackOverflow {
                depth: self.call_depth,
            });
        }
        self.call_depth += 1;

        // Initialize registers
        let mut registers = vec![Value::Unit; func.register_count as usize];

        // Copy arguments into parameter registers
        for (i, arg) in args.into_iter().enumerate() {
            if i < registers.len() {
                registers[i] = arg;
            }
        }

        // Execute blocks
        let mut block_idx: usize = 0;
        let result = loop {
            if block_idx >= func.blocks.len() {
                break Err(VmError::InvalidBlock {
                    function_name: func_name.clone(),
                    block_id: block_idx as u32,
                });
            }

            let block = &func.blocks[block_idx];

            // Execute instructions
            for inst in &block.instructions {
                self.step_count += 1;
                if self.step_count > self.step_limit {
                    break;
                }
                self.execute_instruction(program, func_map, inst, &mut registers)?;
            }

            if self.step_count > self.step_limit {
                break Err(VmError::StepLimitExceeded {
                    limit: self.step_limit,
                });
            }

            // Execute terminator
            match &block.terminator {
                Terminator::Return(reg) => {
                    break Ok(registers[reg.0 as usize].clone());
                }
                Terminator::Jump(target) => {
                    block_idx = func
                        .blocks
                        .iter()
                        .position(|b| b.id == *target)
                        .unwrap_or(target.0 as usize);
                }
                Terminator::Branch(cond, then_block, else_block) => {
                    let cond_val = &registers[cond.0 as usize];
                    let target = if is_truthy(cond_val) {
                        then_block
                    } else {
                        else_block
                    };
                    block_idx = func
                        .blocks
                        .iter()
                        .position(|b| b.id == *target)
                        .unwrap_or(target.0 as usize);
                }
                Terminator::Unreachable => {
                    break Err(VmError::Unreachable);
                }
            }
        };

        self.call_depth -= 1;
        result
    }
}
