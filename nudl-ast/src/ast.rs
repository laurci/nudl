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
}

#[derive(Debug)]
pub enum Literal {
    String(String),
    Int(String),
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
