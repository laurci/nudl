use crate::{Value, VmError};

/// Perform an arithmetic binary operation, dispatching on value types.
pub(crate) fn vm_binop_arith(
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
pub(crate) fn vm_binop_cmp(
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
        (Value::Bool(a), Value::Bool(b)) => {
            Ok(int_op(if *a { 1 } else { 0 }, if *b { 1 } else { 0 }))
        }
        _ => Err(VmError::TypeError {
            message: format!("incompatible types for comparison: {:?} and {:?}", lhs, rhs),
        }),
    }
}

/// Check if a value is truthy (for branch conditions).
pub(crate) fn is_truthy(val: &Value) -> bool {
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
        Value::DynArrayRef(_) => true,
        Value::MapRef(_) => true,
        Value::Dyn(_, _) => true,
    }
}
