use nudl_core::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Literals
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    CharLiteral,
    BoolLiteral,

    // Template string tokens: `text {expr} text`
    TemplateStringStart, // opening ` up to first {
    TemplateStringPart,  // text between } and next {
    TemplateStringEnd,   // text after last } up to closing `

    // Identifier
    Ident,

    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    While,
    For,
    In,
    Loop,
    Break,
    Continue,
    Return,
    Struct,
    Enum,
    Impl,
    Interface,
    Pub,
    Use,
    Mod,
    As,
    Type,
    Const,
    Static,
    Comptime,
    Extern,
    True,
    False,
    Self_,
    SelfType,
    Dyn,
    Match,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Bang,
    Lt,
    Gt,
    Eq,
    EqEq,
    BangEq,
    LtEq,
    GtEq,
    AmpAmp,
    PipePipe,
    LtLt,
    GtGt,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    LtLtEq,
    GtGtEq,
    Arrow,    // ->
    FatArrow, // =>
    DotDot,   // ..
    DotDotEq, // ..=

    // Punctuation
    Dot,
    Comma,
    Colon,
    ColonColon,
    Semi,
    Hash,
    At,
    Question,
    Underscore,

    // Special
    Eof,
    Error,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span, text: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            text: text.into(),
        }
    }
}

pub fn keyword_from_str(s: &str) -> Option<TokenKind> {
    match s {
        "fn" => Some(TokenKind::Fn),
        "let" => Some(TokenKind::Let),
        "mut" => Some(TokenKind::Mut),
        "if" => Some(TokenKind::If),
        "else" => Some(TokenKind::Else),
        "while" => Some(TokenKind::While),
        "for" => Some(TokenKind::For),
        "in" => Some(TokenKind::In),
        "loop" => Some(TokenKind::Loop),
        "break" => Some(TokenKind::Break),
        "continue" => Some(TokenKind::Continue),
        "return" => Some(TokenKind::Return),
        "struct" => Some(TokenKind::Struct),
        "enum" => Some(TokenKind::Enum),
        "impl" => Some(TokenKind::Impl),
        "interface" => Some(TokenKind::Interface),
        "pub" => Some(TokenKind::Pub),
        "use" => Some(TokenKind::Use),
        "mod" => Some(TokenKind::Mod),
        "as" => Some(TokenKind::As),
        "type" => Some(TokenKind::Type),
        "const" => Some(TokenKind::Const),
        "static" => Some(TokenKind::Static),
        "comptime" => Some(TokenKind::Comptime),
        "extern" => Some(TokenKind::Extern),
        "true" => Some(TokenKind::True),
        "false" => Some(TokenKind::False),
        "self" => Some(TokenKind::Self_),
        "Self" => Some(TokenKind::SelfType),
        "dyn" => Some(TokenKind::Dyn),
        "match" => Some(TokenKind::Match),
        _ => None,
    }
}
