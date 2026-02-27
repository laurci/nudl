use std::collections::HashMap;

use nudl_bc::ir::*;
use nudl_core::intern::Symbol;

use crate::helpers::{vm_binop_arith, vm_binop_cmp};
use crate::types::HeapObject;
use crate::vm::Vm;
use crate::{Value, VmError};

impl Vm {
    pub(crate) fn execute_instruction(
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
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a + b,
                    |a, b| a + b,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Sub(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a - b,
                    |a, b| a - b,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Mul(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a * b,
                    |a, b| a * b,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Div(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| if b != 0 { a / b } else { 0 },
                    |a, b| if b != 0.0 { a / b } else { 0.0 },
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Mod(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| if b != 0 { a % b } else { 0 },
                    |a, b| if b != 0.0 { a % b } else { 0.0 },
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Shl(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a << (b & 0x3F),
                    |_a, _b| 0.0,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Shr(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a >> (b & 0x3F),
                    |_a, _b| 0.0,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitAnd(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a & b,
                    |_a, _b| 0.0,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitOr(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a | b,
                    |_a, _b| 0.0,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::BitXor(dst, lhs, rhs) => {
                let result = vm_binop_arith(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a ^ b,
                    |_a, _b| 0.0,
                )?;
                registers[dst.0 as usize] = result;
            }
            Instruction::Neg(dst, src) => {
                let result = match &registers[src.0 as usize] {
                    Value::I32(v) => Value::I32(-*v),
                    Value::I64(v) => Value::I64(-*v),
                    Value::F64(v) => Value::F64(-*v),
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("cannot negate {:?}", other),
                        });
                    }
                };
                registers[dst.0 as usize] = result;
            }
            Instruction::BitNot(dst, src) => {
                let result = match &registers[src.0 as usize] {
                    Value::I32(v) => Value::I32(!*v),
                    Value::I64(v) => Value::I64(!*v),
                    Value::U64(v) => Value::U64(!*v),
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("cannot bitwise-not {:?}", other),
                        });
                    }
                };
                registers[dst.0 as usize] = result;
            }

            // Comparison
            Instruction::Eq(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a == b,
                    |a, b| a == b,
                )?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Ne(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a != b,
                    |a, b| a != b,
                )?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Lt(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a < b,
                    |a, b| a < b,
                )?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Le(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a <= b,
                    |a, b| a <= b,
                )?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Gt(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a > b,
                    |a, b| a > b,
                )?;
                registers[dst.0 as usize] = Value::Bool(result);
            }
            Instruction::Ge(dst, lhs, rhs) => {
                let result = vm_binop_cmp(
                    &registers[lhs.0 as usize],
                    &registers[rhs.0 as usize],
                    |a, b| a >= b,
                    |a, b| a >= b,
                )?;
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
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("cannot negate {:?}", other),
                        });
                    }
                };
                registers[dst.0 as usize] = result;
            }

            // ARC / heap operations
            Instruction::Alloc(dst, type_id) => {
                let id = self.next_heap_id;
                self.next_heap_id += 1;
                self.heap.insert(
                    id,
                    HeapObject {
                        fields: Vec::new(),
                        strong_count: 1,
                        weak_count: 0,
                        type_tag: type_id.0,
                    },
                );
                registers[dst.0 as usize] = Value::HeapRef(id);
            }
            Instruction::Load(dst, ptr_reg, offset) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("Load expected HeapRef, got {:?}", other),
                        });
                    }
                };
                let obj = self.heap.get(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("Load from freed heap object {}", id),
                })?;
                let val = obj
                    .fields
                    .get(*offset as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                registers[dst.0 as usize] = val;
            }
            Instruction::Store(ptr_reg, offset, src) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("Store expected HeapRef, got {:?}", other),
                        });
                    }
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
                    obj.strong_count =
                        obj.strong_count
                            .checked_add(1)
                            .ok_or_else(|| VmError::TypeError {
                                message: "ARC strong count overflow".into(),
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

            // Tuple/Array operations -- use same heap object representation
            Instruction::TupleAlloc(dst, type_id, elements)
            | Instruction::FixedArrayAlloc(dst, type_id, elements) => {
                let id = self.next_heap_id;
                self.next_heap_id += 1;
                let fields: Vec<Value> = elements
                    .iter()
                    .map(|r| registers[r.0 as usize].clone())
                    .collect();
                self.heap.insert(
                    id,
                    HeapObject {
                        fields,
                        strong_count: 1,
                        weak_count: 0,
                        type_tag: type_id.0,
                    },
                );
                registers[dst.0 as usize] = Value::HeapRef(id);
            }
            Instruction::TupleLoad(dst, ptr_reg, offset) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("TupleLoad expected HeapRef, got {:?}", other),
                        });
                    }
                };
                let obj = self.heap.get(&id).ok_or_else(|| VmError::TypeError {
                    message: format!("TupleLoad from freed heap object {}", id),
                })?;
                let val = obj
                    .fields
                    .get(*offset as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                registers[dst.0 as usize] = val;
            }
            Instruction::TupleStore(ptr_reg, offset, src) => {
                let id = match &registers[ptr_reg.0 as usize] {
                    Value::HeapRef(id) => *id,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("TupleStore expected HeapRef, got {:?}", other),
                        });
                    }
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
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("IndexLoad expected HeapRef, got {:?}", other),
                        });
                    }
                };
                let idx = match &registers[idx_reg.0 as usize] {
                    Value::I32(v) => *v as usize,
                    Value::I64(v) => *v as usize,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("IndexLoad expected integer index, got {:?}", other),
                        });
                    }
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
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("IndexStore expected HeapRef, got {:?}", other),
                        });
                    }
                };
                let idx = match &registers[idx_reg.0 as usize] {
                    Value::I32(v) => *v as usize,
                    Value::I64(v) => *v as usize,
                    other => {
                        return Err(VmError::TypeError {
                            message: format!("IndexStore expected integer index, got {:?}", other),
                        });
                    }
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

            // Closure, dynamic array, and map instructions are not supported in the VM (comptime)
            // They are only used in the native backend. Emit no-ops or errors.
            Instruction::ClosureCreate(dst, _, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::ClosureCall(dst, _, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::DynArrayAlloc(dst, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::DynArrayPush(_, _) => {}
            Instruction::DynArrayPop(dst, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::DynArrayLen(dst, _) => {
                registers[dst.0 as usize] = Value::I64(0);
            }
            Instruction::DynArrayGet(dst, _, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::DynArraySet(_, _, _) => {}
            Instruction::MapAlloc(dst, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::MapInsert(_, _, _) => {}
            Instruction::MapGet(dst, _, _) => {
                registers[dst.0 as usize] = Value::Unit;
            }
            Instruction::MapLen(dst, _) => {
                registers[dst.0 as usize] = Value::I64(0);
            }
            Instruction::MapContains(dst, _, _) => {
                registers[dst.0 as usize] = Value::Bool(false);
            }
        }

        Ok(())
    }
}
