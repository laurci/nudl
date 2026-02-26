use std::collections::HashMap;
use std::fmt;

use nudl_bc::ir::*;
use nudl_core::intern::Symbol;

/// Simulated heap object for ARC in the VM.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct HeapObject {
    /// Field values (each slot is 8 bytes / one Value)
    fields: Vec<Value>,
    strong_count: u32,
    weak_count: u32,
    type_tag: u32,
}

/// Runtime value in the VM.
#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    I32(i32),
    I64(i64),
    U64(u64),
    Bool(bool),
    F64(f64),
    Char(char),
    /// String constant (index into Program::string_constants).
    String(u32),
    /// Synthetic raw pointer (not dereferenceable, only for VM-internal tracking).
    RawPtr(u64),
    /// ARC heap object reference (index into Vm::heap).
    HeapRef(u64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unit => write!(f, "()"),
            Value::I32(v) => write!(f, "{}", v),
            Value::I64(v) => write!(f, "{}", v),
            Value::U64(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", v),
            Value::F64(v) => write!(f, "{}", v),
            Value::Char(v) => write!(f, "{}", v),
            Value::String(idx) => write!(f, "string[{}]", idx),
            Value::RawPtr(v) => write!(f, "ptr(0x{:x})", v),
            Value::HeapRef(id) => write!(f, "heap({})", id),
        }
    }
}

/// VM execution error.
#[derive(Debug)]
pub enum VmError {
    /// Attempted to call an extern function, which is not allowed in the VM.
    ExternCallNotAllowed { function_name: String },
    /// Function not found.
    UndefinedFunction { symbol: Symbol },
    /// Execution exceeded the step limit.
    StepLimitExceeded { limit: u64 },
    /// Hit an unreachable terminator.
    Unreachable,
    /// No entry function (main) found.
    NoEntryFunction,
    /// Invalid block index.
    InvalidBlock {
        function_name: String,
        block_id: u32,
    },
    /// Stack overflow (too many nested calls).
    StackOverflow { depth: usize },
    /// Type error at runtime (shouldn't happen with proper type checking).
    TypeError { message: String },
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmError::ExternCallNotAllowed { function_name } => write!(
                f,
                "cannot call extern function '{}' in the VM",
                function_name
            ),
            VmError::UndefinedFunction { symbol } => {
                write!(f, "undefined function (symbol {})", symbol.0)
            }
            VmError::StepLimitExceeded { limit } => {
                write!(f, "execution exceeded step limit of {}", limit)
            }
            VmError::Unreachable => write!(f, "hit unreachable code"),
            VmError::NoEntryFunction => write!(f, "no entry function (main) found"),
            VmError::InvalidBlock {
                function_name,
                block_id,
            } => write!(
                f,
                "invalid block b{} in function '{}'",
                block_id, function_name
            ),
            VmError::StackOverflow { depth } => write!(f, "stack overflow at depth {}", depth),
            VmError::TypeError { message } => write!(f, "type error: {}", message),
        }
    }
}

const DEFAULT_STEP_LIMIT: u64 = 1_000_000;
const MAX_CALL_DEPTH: usize = 256;

/// Register-based SSA bytecode interpreter.
pub struct Vm {
    step_count: u64,
    step_limit: u64,
    call_depth: usize,
    /// Simulated heap for ARC objects (comptime).
    heap: HashMap<u64, HeapObject>,
    next_heap_id: u64,
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

    fn execute_function(
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

    fn execute_instruction(
        &mut self,
        program: &Program,
        func_map: &HashMap<Symbol, usize>,
        inst: &Instruction,
        registers: &mut [Value],
    ) -> Result<(), VmError> {
        match inst {
            Instruction::Const(reg, val) => {
                registers[reg.0 as usize] = match val {
                    ConstValue::Unit => Value::Unit,
                    ConstValue::I32(v) => Value::I32(*v),
                    ConstValue::I64(v) => Value::I64(*v),
                    ConstValue::U64(v) => Value::U64(*v),
                    ConstValue::Bool(v) => Value::Bool(*v),
                    ConstValue::F32(v) => Value::F64(*v as f64),
                    ConstValue::F64(v) => Value::F64(*v),
                    ConstValue::Char(v) => Value::Char(*v),
                    ConstValue::StringLiteral(idx) => Value::String(*idx),
                };
            }

            Instruction::ConstUnit(reg) => {
                registers[reg.0 as usize] = Value::Unit;
            }

            Instruction::StringPtr(dst, src) => {
                let val = match &registers[src.0 as usize] {
                    Value::String(idx) => Value::RawPtr(*idx as u64),
                    _ => Value::RawPtr(0),
                };
                registers[dst.0 as usize] = val;
            }

            Instruction::StringLen(dst, src) => {
                let val = match &registers[src.0 as usize] {
                    Value::String(idx) => {
                        let len = program
                            .string_constants
                            .get(*idx as usize)
                            .map(|s| s.len() as u64)
                            .unwrap_or(0);
                        Value::U64(len)
                    }
                    _ => Value::U64(0),
                };
                registers[dst.0 as usize] = val;
            }

            Instruction::StringConstPtr(dst, idx) => {
                registers[dst.0 as usize] = Value::RawPtr(*idx as u64);
            }

            Instruction::StringConstLen(dst, idx) => {
                let len = program
                    .string_constants
                    .get(*idx as usize)
                    .map(|s| s.len() as u64)
                    .unwrap_or(0);
                registers[dst.0 as usize] = Value::U64(len);
            }

            Instruction::Call(dst, func_ref, args) => {
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|r| registers[r.0 as usize].clone())
                    .collect();

                match func_ref {
                    FunctionRef::Named(sym) => {
                        let idx = func_map
                            .get(sym)
                            .ok_or(VmError::UndefinedFunction { symbol: *sym })?;
                        let result = self.execute_function(program, func_map, *idx, arg_values)?;
                        registers[dst.0 as usize] = result;
                    }
                    FunctionRef::Extern(sym) => {
                        let name = program.interner.resolve(*sym);
                        return Err(VmError::ExternCallNotAllowed {
                            function_name: name.to_string(),
                        });
                    }
                    FunctionRef::Builtin(sym) => {
                        let name = program.interner.resolve(*sym);
                        match name {
                            "__str_ptr" => {
                                let val = match arg_values.first() {
                                    Some(Value::String(idx)) => Value::RawPtr(*idx as u64),
                                    _ => Value::RawPtr(0),
                                };
                                registers[dst.0 as usize] = val;
                            }
                            "__str_len" => {
                                let val = match arg_values.first() {
                                    Some(Value::String(idx)) => {
                                        let len = program
                                            .string_constants
                                            .get(*idx as usize)
                                            .map(|s| s.len() as u64)
                                            .unwrap_or(0);
                                        Value::U64(len)
                                    }
                                    _ => Value::U64(0),
                                };
                                registers[dst.0 as usize] = val;
                            }
                            _ => {
                                registers[dst.0 as usize] = Value::Unit;
                            }
                        }
                    }
                }
            }

            Instruction::Copy(dst, src) => {
                registers[dst.0 as usize] = registers[src.0 as usize].clone();
            }

            Instruction::Nop => {}

            // Arithmetic
            Instruction::Add(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a + b, |a, b| a + b)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Sub(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a - b, |a, b| a - b)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Mul(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a * b, |a, b| a * b)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Div(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| if b != 0 { a / b } else { 0 }, |a, b| if b != 0.0 { a / b } else { 0.0 })?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Mod(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| if b != 0 { a % b } else { 0 }, |a, b| if b != 0.0 { a % b } else { 0.0 })?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Shl(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a << (b & 0x3F), |_a, _b| 0.0)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Shr(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a >> (b & 0x3F), |_a, _b| 0.0)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitAnd(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a & b, |_a, _b| 0.0)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitOr(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a | b, |_a, _b| 0.0)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitXor(dst, lhs, rhs) => {
                let result = vm_binop_arith(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a ^ b, |_a, _b| 0.0)?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Neg(dst, src) => {
                let result = match &registers[src.0 as usize] {
                    Value::I32(v) => Value::I32(-*v),
                    Value::I64(v) => Value::I64(-*v),
                    Value::F64(v) => Value::F64(-*v),
                    other => return Err(VmError::TypeError { message: format!("cannot negate {:?}", other) }),
                };
                registers[dst.0 as usize] = result;
            }
            Instruction::BitNot(dst, src) => {
                let result = match &registers[src.0 as usize] {
                    Value::I32(v) => Value::I32(!*v),
                    Value::I64(v) => Value::I64(!*v),
                    Value::U64(v) => Value::U64(!*v),
                    other => return Err(VmError::TypeError { message: format!("cannot bitwise-not {:?}", other) }),
                };
                registers[dst.0 as usize] = result;
            }

            // Comparison
            Instruction::Eq(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a == b, |a, b| a == b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Ne(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a != b, |a, b| a != b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Lt(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a < b, |a, b| a < b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Le(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a <= b, |a, b| a <= b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Gt(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a > b, |a, b| a > b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Ge(dst, lhs, rhs) => {
                let result = vm_binop_cmp(&registers[lhs.0 as usize], &registers[rhs.0 as usize], |a, b| a >= b, |a, b| a >= b)?;
                registers[dst.0 as usize] = Value::Bool(result);
            }

            // Cast (no-op in VM for now - all values carry their types)
            Instruction::Cast(dst, src, _target_type) => {
                registers[dst.0 as usize] = registers[src.0 as usize].clone();
            }

            // Logical
            Instruction::Not(dst, src) => {
                let result = match &registers[src.0 as usize] {
                    Value::Bool(v) => Value::Bool(!*v),
                    other => return Err(VmError::TypeError { message: format!("cannot negate {:?}", other) }),
                };
                registers[dst.0 as usize] = result;
            }

            // ARC / heap operations
            Instruction::Alloc(dst, type_id) => {
                let id = self.next_heap_id;
                self.next_heap_id += 1;
                self.heap.insert(id, HeapObject {
                    fields: Vec::new(),
                    strong_count: 1,
                    weak_count: 0,
                    type_tag: type_id.0,
                });
                registers[dst.0 as usize] = Value::HeapRef(id);
            }
            Instruction::Load(dst, ptr_reg, offset) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("Load expected HeapRef, got {:?}", other),
                    }),
                };
                let obj = self.heap.get(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("Load from freed heap object {}", id),
                })?;
                let val = obj.fields.get(*offset as usize).cloned().unwrap_or(Value::Unit);
                registers[dst.0 as usize] = val;
            }
            Instruction::Store(ptr_reg, offset, src) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("Store expected HeapRef, got {:?}", other),
                    }),
                };
                let val = registers[src.0 as usize].clone();
                let obj = self.heap.get_mut(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("Store to freed heap object {}", id),
                })?;
                let idx = *offset as usize;
                if idx >= obj.fields.len() {
                    obj.fields.resize(idx + 1, Value::Unit);
                }
                obj.fields[idx] = val;
            }
            Instruction::Retain(reg) => {
                let id = match &registers[reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    _ => return Ok(()), // null / non-ref: no-op
                };
                if let Some(obj) = self.heap.get_mut(&id) {
                    obj.strong_count = obj.strong_count.checked_add(1).ok_or_else(|| {
                        VmError::TypeError { message: "ARC strong count overflow".into() }
                    })?;
                }
            }
            Instruction::Release(reg, _type_id) => {
                let id = match &registers[reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    _ => return Ok(()), // null / non-ref: no-op
                };
                let should_free = if let Some(obj) = self.heap.get_mut(&id) {
                    obj.strong_count = obj.strong_count.saturating_sub(1);
                    obj.strong_count == 0 && obj.weak_count == 0
                } else {
                    false
                };
                if should_free {
                    self.heap.remove(&id);
                }
            }

            // Tuple/Array operations — use same heap object representation
            Instruction::TupleAlloc(dst, type_id, elements) | Instruction::FixedArrayAlloc(dst, type_id, elements) => {
                let id = self.next_heap_id;
                self.next_heap_id += 1;
                let fields: Vec<Value> = elements.iter().map(|r| registers[r.0 as usize].clone()).collect();
                self.heap.insert(id, HeapObject {
                    fields,
                    strong_count: 1,
                    weak_count: 0,
                    type_tag: type_id.0,
                });
                registers[dst.0 as usize] = Value::HeapRef(id);
            }
            Instruction::TupleLoad(dst, ptr_reg, offset) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("TupleLoad expected HeapRef, got {:?}", other),
                    }),
                };
                let obj = self.heap.get(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("TupleLoad from freed heap object {}", id),
                })?;
                let val = obj.fields.get(*offset as usize).cloned().unwrap_or(Value::Unit);
                registers[dst.0 as usize] = val;
            }
            Instruction::TupleStore(ptr_reg, offset, src) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("TupleStore expected HeapRef, got {:?}", other),
                    }),
                };
                let val = registers[src.0 as usize].clone();
                let obj = self.heap.get_mut(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("TupleStore to freed heap object {}", id),
                })?;
                let idx = *offset as usize;
                if idx >= obj.fields.len() {
                    obj.fields.resize(idx + 1, Value::Unit);
                }
                obj.fields[idx] = val;
            }
            Instruction::IndexLoad(dst, ptr_reg, idx_reg, _elem_type) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("IndexLoad expected HeapRef, got {:?}", other),
                    }),
                };
                let idx = match &registers[idx_reg.0 as usize] {
                    Value::I32(v) => *v as usize,
                    Value::I64(v) => *v as usize,
                    other => return Err(VmError::TypeError {
                        message: format!("IndexLoad expected integer index, got {:?}", other),
                    }),
                };
                let obj = self.heap.get(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("IndexLoad from freed heap object {}", id),
                })?;
                let val = obj.fields.get(idx).cloned().unwrap_or(Value::Unit);
                registers[dst.0 as usize] = val;
            }
            Instruction::IndexStore(ptr_reg, idx_reg, src) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => return Err(VmError::TypeError {
                        message: format!("IndexStore expected HeapRef, got {:?}", other),
                    }),
                };
                let idx = match &registers[idx_reg.0 as usize] {
                    Value::I32(v) => *v as usize,
                    Value::I64(v) => *v as usize,
                    other => return Err(VmError::TypeError {
                        message: format!("IndexStore expected integer index, got {:?}", other),
                    }),
                };
                let val = registers[src.0 as usize].clone();
                let obj = self.heap.get_mut(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("IndexStore to freed heap object {}", id),
                })?;
                if idx >= obj.fields.len() {
                    obj.fields.resize(idx + 1, Value::Unit);
                }
                obj.fields[idx] = val;
            }
        }

        Ok(())
    }
}

/// Perform an arithmetic binary operation, dispatching on value types.
fn vm_binop_arith(
    lhs: &Value,
    rhs: &Value,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, VmError> {
    match (lhs, rhs) {
        (Value::I32(a), Value::I32(b)) => Ok(Value::I32(int_op(*a as i64, *b as i64) as i32)),
        (Value::I64(a), Value::I64(b)) => Ok(Value::I64(int_op(*a, *b))),
        (Value::U64(a), Value::U64(b)) => Ok(Value::U64(int_op(*a as i64, *b as i64) as u64)),
        (Value::F64(a), Value::F64(b)) => Ok(Value::F64(float_op(*a, *b))),
        _ => Err(VmError::TypeError {
            message: format!("incompatible types for arithmetic: {:?} and {:?}", lhs, rhs),
        }),
    }
}

/// Perform a comparison binary operation, dispatching on value types.
fn vm_binop_cmp(
    lhs: &Value,
    rhs: &Value,
    int_op: impl Fn(i64, i64) -> bool,
    float_op: impl Fn(f64, f64) -> bool,
) -> Result<bool, VmError> {
    match (lhs, rhs) {
        (Value::I32(a), Value::I32(b)) => Ok(int_op(*a as i64, *b as i64)),
        (Value::I64(a), Value::I64(b)) => Ok(int_op(*a, *b)),
        (Value::U64(a), Value::U64(b)) => Ok(int_op(*a as i64, *b as i64)),
        (Value::F64(a), Value::F64(b)) => Ok(float_op(*a, *b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(int_op(if *a { 1 } else { 0 }, if *b { 1 } else { 0 })),
        _ => Err(VmError::TypeError {
            message: format!("incompatible types for comparison: {:?} and {:?}", lhs, rhs),
        }),
    }
}

/// Check if a value is truthy (for branch conditions).
fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Unit => false,
        Value::I32(v) => *v != 0,
        Value::I64(v) => *v != 0,
        Value::U64(v) => *v != 0,
        Value::Bool(v) => *v,
        Value::F64(v) => *v != 0.0,
        Value::Char(v) => *v != '\0',
        Value::String(_) => true,
        Value::RawPtr(v) => *v != 0,
        Value::HeapRef(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_core::span::FileId;

    fn compile(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        Lowerer::new(checked).lower(&module)
    }

    #[test]
    fn run_empty_main() {
        let program = compile("fn main() {}");
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Value::Unit));
    }

    #[test]
    fn run_function_call() {
        let program = compile(
            r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn run_nested_calls() {
        let program = compile(
            r#"
fn inner(s: string) {}
fn outer(s: string) {
    inner(s);
}
fn main() {
    outer("hello");
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn extern_call_fails() {
        let program = compile(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}
fn main() {
    print("hello");
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_err());
        match result.unwrap_err() {
            VmError::ExternCallNotAllowed { function_name } => {
                assert_eq!(function_name, "write");
            }
            other => panic!("expected ExternCallNotAllowed, got {:?}", other),
        }
    }

    #[test]
    fn step_limit_exceeded() {
        let program = compile(
            r#"
fn a(s: string) {}
fn b(s: string) { a(s); }
fn c(s: string) { b(s); }
fn d(s: string) { c(s); }
fn main() {
    d("x");
}
"#,
        );
        let mut vm = Vm::with_step_limit(5);
        let result = vm.run(&program);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VmError::StepLimitExceeded { .. }
        ));
    }

    #[test]
    fn string_builtins_work() {
        let program = compile(
            r#"
fn main() {
    __str_ptr("hello");
    __str_len("hello");
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn no_entry_function_error() {
        let program = Program {
            functions: vec![],
            string_constants: vec![],
            entry_function: None,
            extern_libs: vec![],
            interner: nudl_core::intern::StringInterner::new(),
            types: nudl_core::types::TypeInterner::new(),
            source_map: None,
        };
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::NoEntryFunction));
    }

    #[test]
    fn vm_arithmetic() {
        let program = compile(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    let result = add(10, 20);
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn vm_if_else() {
        let program = compile(
            r#"
fn pick(x: i32) -> i32 {
    if x > 5 { 1 } else { 0 }
}
fn main() {
    let a = pick(10);
    let b = pick(3);
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn vm_while_loop() {
        let program = compile(
            r#"
fn main() {
    let mut x: i32 = 0;
    while x < 10 {
        x = x + 1;
    }
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn vm_loop_break() {
        let program = compile(
            r#"
fn main() {
    let mut x: i32 = 0;
    loop {
        x = x + 1;
        if x > 5 {
            break;
        }
    }
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn vm_function_return_value() {
        let program = compile(
            r#"
fn double(x: i32) -> i32 {
    x + x
}
fn main() {
    let a = double(21);
}
"#,
        );
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }
}
