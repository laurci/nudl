use nudl_core::span::{Span, Spanned};

pub type SpannedItem = Spanned<Item>;
pub type SpannedExpr = Spanned<Expr>;
pub type SpannedStmt = Spanned<Stmt>;

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<SpannedItem>,
}

#[derive(Debug, Clone)]
pub enum Item {
    FnDef {
        name: String,
        type_params: Vec<TypeParam>,
        params: Vec<Param>,
        return_type: Option<Spanned<TypeExpr>>,
        body: Spanned<Block>,
        is_pub: bool,
    },
    StructDef {
        name: String,
        type_params: Vec<TypeParam>,
        fields: Vec<StructField>,
        is_pub: bool,
    },
    EnumDef {
        name: String,
        type_params: Vec<TypeParam>,
        variants: Vec<EnumVariantDef>,
        is_pub: bool,
    },
    InterfaceDef {
        name: String,
        type_params: Vec<TypeParam>,
        methods: Vec<InterfaceMethodDef>,
        is_pub: bool,
    },
    ImplBlock {
        type_name: String,
        type_args: Vec<Spanned<TypeExpr>>,
        interface_name: Option<String>,
        methods: Vec<SpannedItem>,
    },
    ExternBlock {
        library: Option<String>,
        items: Vec<Spanned<ExternFnDecl>>,
    },
    /// Import statement: `import std::io;` or `import std::io::{print, println};`
    Import {
        path: Vec<String>,          // e.g., ["std", "io"]
        items: Option<Vec<String>>, // None = import whole module, Some = specific items
        alias: Option<String>,      // e.g., `as p`
    },
    /// Type alias: `type Name = ExistingType;`
    TypeAlias {
        name: String,
        ty: Spanned<TypeExpr>,
        is_pub: bool,
    },
}

#[derive(Debug, Clone)]
pub struct TypeParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDef {
    pub name: String,
    pub kind: VariantKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum VariantKind {
    Unit,
    Tuple(Vec<Spanned<TypeExpr>>),
    Struct(Vec<StructField>),
}

#[derive(Debug, Clone)]
pub struct InterfaceMethodDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub is_mut: bool,
    pub is_self: bool,
    pub default_value: Option<Box<SpannedExpr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExternFnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<SpannedStmt>,
    pub tail_expr: Option<Box<SpannedExpr>>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(SpannedExpr),
    Let {
        name: String,
        ty: Option<Spanned<TypeExpr>>,
        value: SpannedExpr,
        is_mut: bool,
    },
    /// Pattern-based let: `let (a, b) = expr;` or `let Foo { x, y } = expr;`
    LetPattern {
        pattern: Spanned<Pattern>,
        ty: Option<Spanned<TypeExpr>>,
        value: SpannedExpr,
        is_mut: bool,
    },
    Const {
        name: String,
        ty: Option<Spanned<TypeExpr>>,
        value: SpannedExpr,
    },
    /// Defer statement: `defer { ... }`
    Defer {
        body: Spanned<Block>,
    },
    Item(SpannedItem),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Call {
        callee: Box<SpannedExpr>,
        args: Vec<CallArg>,
    },
    Block(Block),
    Return(Option<Box<SpannedExpr>>),
    Binary {
        op: BinOp,
        left: Box<SpannedExpr>,
        right: Box<SpannedExpr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<SpannedExpr>,
    },
    Assign {
        target: Box<SpannedExpr>,
        value: Box<SpannedExpr>,
    },
    CompoundAssign {
        op: BinOp,
        target: Box<SpannedExpr>,
        value: Box<SpannedExpr>,
    },
    If {
        condition: Box<SpannedExpr>,
        then_branch: Box<Spanned<Block>>,
        else_branch: Option<Box<SpannedExpr>>,
    },
    Cast {
        expr: Box<SpannedExpr>,
        target_type: Spanned<TypeExpr>,
    },
    While {
        label: Option<String>,
        condition: Box<SpannedExpr>,
        body: Box<Spanned<Block>>,
    },
    Loop {
        label: Option<String>,
        body: Box<Spanned<Block>>,
    },
    Break {
        label: Option<String>,
        value: Option<Box<SpannedExpr>>,
    },
    Continue {
        label: Option<String>,
    },
    Grouped(Box<SpannedExpr>),
    StructLiteral {
        name: String,
        fields: Vec<(String, SpannedExpr)>,
    },
    FieldAccess {
        object: Box<SpannedExpr>,
        field: String,
    },
    MethodCall {
        object: Box<SpannedExpr>,
        method: String,
        args: Vec<CallArg>,
    },
    StaticCall {
        type_name: String,
        method: String,
        args: Vec<CallArg>,
    },
    TupleLiteral(Vec<SpannedExpr>),
    ArrayLiteral(Vec<SpannedExpr>),
    ArrayRepeat {
        value: Box<SpannedExpr>,
        count: usize,
    },
    IndexAccess {
        object: Box<SpannedExpr>,
        index: Box<SpannedExpr>,
    },
    Range {
        start: Box<SpannedExpr>,
        end: Box<SpannedExpr>,
        inclusive: bool,
    },
    For {
        binding: String,
        iter: Box<SpannedExpr>,
        body: Box<Spanned<Block>>,
    },
    /// Enum variant construction: `Color::Red`, `Shape::Circle(5.0)`
    EnumLiteral {
        enum_name: String,
        variant: String,
        args: Vec<SpannedExpr>,
    },
    /// Match expression
    Match {
        expr: Box<SpannedExpr>,
        arms: Vec<MatchArm>,
    },
    /// if let pattern = expr { ... } else { ... }
    IfLet {
        pattern: Spanned<Pattern>,
        expr: Box<SpannedExpr>,
        then_branch: Box<Spanned<Block>>,
        else_branch: Option<Box<SpannedExpr>>,
    },
    /// Closure: `|params| body` or `|params| -> RetType { body }`
    Closure {
        params: Vec<ClosureParam>,
        return_type: Option<Spanned<TypeExpr>>,
        body: Box<SpannedExpr>,
    },
    /// `?` postfix error propagation: `expr?`
    QuestionMark(Box<SpannedExpr>),
}

#[derive(Debug, Clone)]
pub struct ClosureParam {
    pub name: String,
    pub ty: Option<Spanned<TypeExpr>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntSuffix {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

#[derive(Debug, Clone)]
pub enum Literal {
    String(String),
    /// Template string: alternating text parts and expression parts.
    /// `parts` has one more element than `exprs` (text before first expr,
    /// between exprs, and after last expr).
    TemplateString {
        parts: Vec<String>,
        exprs: Vec<SpannedExpr>,
    },
    Int(String, Option<IntSuffix>),
    Float(String),
    Bool(bool),
    Char(char),
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: SpannedExpr,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<SpannedExpr>,
    pub body: SpannedExpr,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    Literal(Literal),
    Tuple(Vec<Spanned<Pattern>>),
    Enum {
        enum_name: Option<String>,
        variant: String,
        fields: Vec<Spanned<Pattern>>,
    },
    Struct {
        name: String,
        fields: Vec<(String, Spanned<Pattern>)>, // (field_name, pattern)
        has_rest: bool,                          // true if `..` is present
    },
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Named(String),
    Unit,
    Tuple(Vec<Spanned<TypeExpr>>),
    FixedArray {
        element: Box<Spanned<TypeExpr>>,
        length: usize,
    },
    /// Generic type: `Map<K, V>`, `Option<T>`, etc.
    Generic {
        name: String,
        args: Vec<Spanned<TypeExpr>>,
    },
    /// Dynamic array type: `T[]`
    DynamicArray {
        element: Box<Spanned<TypeExpr>>,
    },
    /// Dynamic interface type: `dyn Interface`
    DynInterface {
        name: String,
    },
}
