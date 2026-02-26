use nudl_core::span::{Span, Spanned};

pub type SpannedItem = Spanned<Item>;
pub type SpannedExpr = Spanned<Expr>;
pub type SpannedStmt = Spanned<Stmt>;

#[derive(Debug)]
pub struct Module {
    pub items: Vec<SpannedItem>,
}

#[derive(Debug)]
pub enum Item {
    FnDef {
        name: String,
        params: Vec<Param>,
        return_type: Option<Spanned<TypeExpr>>,
        body: Spanned<Block>,
        is_pub: bool,
    },
    ExternBlock {
        library: Option<String>,
        items: Vec<Spanned<ExternFnDecl>>,
    },
}

#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub is_mut: bool,
    pub span: Span,
}

#[derive(Debug)]
pub struct ExternFnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
}

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<SpannedStmt>,
    pub tail_expr: Option<Box<SpannedExpr>>,
}

#[derive(Debug)]
pub enum Stmt {
    Expr(SpannedExpr),
    Let {
        name: String,
        ty: Option<Spanned<TypeExpr>>,
        value: SpannedExpr,
        is_mut: bool,
    },
    Item(SpannedItem),
}

#[derive(Debug)]
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
    While {
        condition: Box<SpannedExpr>,
        body: Box<Spanned<Block>>,
    },
    Loop {
        body: Box<Spanned<Block>>,
    },
    Break(Option<Box<SpannedExpr>>),
    Continue,
    Grouped(Box<SpannedExpr>),
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: SpannedExpr,
}

#[derive(Debug)]
pub enum TypeExpr {
    Named(String),
    Unit,
}
