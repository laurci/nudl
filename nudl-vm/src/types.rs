use std::fmt;

use nudl_core::intern::Symbol;

/// Simulated heap object for ARC in the VM.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct HeapObject {
    /// Field values (each slot is 8 bytes / one Value)
    pub(crate) fields: Vec<Value>,
    pub(crate) strong_count: u32,
    pub(crate) weak_count: u32,
    pub(crate) type_tag: u32,
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
    /// Dynamic array reference (index into Vm::dyn_arrays).
    DynArrayRef(u64),
    /// Map reference (index into Vm::maps).
    MapRef(u64),
    /// Dynamic dispatch wrapper: (inner_value, vtable_index)
    Dyn(Box<Value>, u32),
}

/// VM-internal dynamic array.
#[derive(Debug, Clone)]
pub(crate) struct VmDynArray {
    pub(crate) elements: Vec<Value>,
}

/// VM-internal map (key-value pairs, linear search).
#[derive(Debug, Clone)]
pub(crate) struct VmMap {
    pub(crate) entries: Vec<(Value, Value)>,
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
            Value::DynArrayRef(id) => write!(f, "dyn_array({})", id),
            Value::MapRef(id) => write!(f, "map({})", id),
            Value::Dyn(inner, vtable_idx) => write!(f, "dyn({}, vtable={})", inner, vtable_idx),
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
