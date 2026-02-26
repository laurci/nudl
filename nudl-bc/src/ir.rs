use nudl_core::intern::{StringInterner, Symbol};
use nudl_core::source::SourceMap;
use nudl_core::span::Span;
use nudl_core::types::{TypeId, TypeInterner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Unit,
    I32(i32),
    I64(i64),
    U64(u64),
    Bool(bool),
    F32(f32),
    F64(f64),
    Char(char),
    StringLiteral(u32), // index into Program::string_constants
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionRef {
    Named(Symbol),
    Extern(Symbol),
    Builtin(Symbol),
}

#[derive(Debug, Clone)]
pub enum Instruction {
    Const(Register, ConstValue),
    ConstUnit(Register),
    StringConstPtr(Register, u32), // register, string_constant_index (legacy, kept for compat)
    StringConstLen(Register, u32), // register, string_constant_index (legacy, kept for compat)
    /// Extract pointer from a string value (works for both literals and params)
    StringPtr(Register, Register), // dst, src_string
    /// Extract length from a string value
    StringLen(Register, Register), // dst, src_string
    Call(Register, FunctionRef, Vec<Register>),
    Copy(Register, Register),
    Nop,

    // Arithmetic
    Add(Register, Register, Register), // dst = lhs + rhs
    Sub(Register, Register, Register),
    Mul(Register, Register, Register),
    Div(Register, Register, Register),
    Mod(Register, Register, Register),
    Shl(Register, Register, Register), // dst = lhs << rhs
    Shr(Register, Register, Register), // dst = lhs >> rhs
    BitAnd(Register, Register, Register), // dst = lhs & rhs
    BitOr(Register, Register, Register),  // dst = lhs | rhs
    BitXor(Register, Register, Register), // dst = lhs ^ rhs
    Neg(Register, Register), // dst = -src
    BitNot(Register, Register), // dst = ~src

    // Comparison
    Eq(Register, Register, Register), // dst = lhs == rhs (bool)
    Ne(Register, Register, Register),
    Lt(Register, Register, Register),
    Le(Register, Register, Register),
    Gt(Register, Register, Register),
    Ge(Register, Register, Register),

    // Logical
    Not(Register, Register), // dst = !src

    // Cast
    Cast(Register, Register, TypeId), // dst = src as target_type

    // ARC / heap operations
    Alloc(Register, TypeId),          // dst = heap-allocate object of given type
    Load(Register, Register, u32),    // dst = ptr.field[offset] (load from heap object)
    Store(Register, u32, Register),   // ptr.field[offset] = src (store into heap object)
    Retain(Register),                 // ++strong_count (ARC retain)
    Release(Register, Option<TypeId>), // --strong_count, free if zero (ARC release); type used for drop fn

    // Tuple/Array operations (stack-allocated value types)
    TupleAlloc(Register, TypeId, Vec<Register>),        // dst = allocate tuple, store elements
    FixedArrayAlloc(Register, TypeId, Vec<Register>),    // dst = allocate fixed array, store elements
    TupleLoad(Register, Register, u32),                  // dst = tuple[offset] (load from stack tuple)
    TupleStore(Register, u32, Register),                 // tuple[offset] = src (store into stack tuple)
    IndexLoad(Register, Register, Register, TypeId),     // dst = array[index] (dynamic index load)
    IndexStore(Register, Register, Register),             // array[index] = value (dynamic index store)
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Register),
    Jump(BlockId),
    Branch(Register, BlockId, BlockId),
    Unreachable,
}

#[derive(Debug)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub spans: Vec<Span>, // parallel to instructions, same length
    pub terminator: Terminator,
}

#[derive(Debug)]
pub struct Function {
    pub id: FunctionId,
    pub name: Symbol,
    pub params: Vec<(Symbol, TypeId)>,
    pub return_type: TypeId,
    pub blocks: Vec<BasicBlock>,
    pub register_count: u32,
    /// TypeId for each register (indexed by Register.0). Defaults to i64.
    pub register_types: Vec<TypeId>,
    pub is_extern: bool,
    pub extern_symbol: Option<String>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
    pub string_constants: Vec<String>,
    pub entry_function: Option<FunctionId>,
    pub extern_libs: Vec<String>,
    pub interner: StringInterner,
    pub types: TypeInterner,
    pub source_map: Option<SourceMap>,
}
