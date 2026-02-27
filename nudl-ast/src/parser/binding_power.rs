use crate::ast::*;
use crate::token::TokenKind;

/// Returns (left_bp, right_bp) for infix operators.
pub(super) fn infix_binding_power(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        // Range: non-associative (between assignment and pipe)
        TokenKind::DotDot | TokenKind::DotDotEq => Some((2, 3)),
        // Pipe: left-associative. Right BP is above bitwise-OR (l_bp=10) so that
        // `|` after the pipe RHS is available for trailing closure syntax, not
        // consumed as a binary operator.  `a |> f |x| { ... }` → `f(a, |x| ...)`
        TokenKind::PipeGt => Some((4, 11)),
        // Logical or: left-associative
        TokenKind::PipePipe => Some((6, 7)),
        // Logical and: left-associative
        TokenKind::AmpAmp => Some((8, 9)),
        // Bitwise or: left-associative
        TokenKind::Pipe => Some((10, 11)),
        // Bitwise xor: left-associative
        TokenKind::Caret => Some((12, 13)),
        // Bitwise and: left-associative
        TokenKind::Amp => Some((14, 15)),
        // Comparison: non-associative
        TokenKind::EqEq
        | TokenKind::BangEq
        | TokenKind::Lt
        | TokenKind::Gt
        | TokenKind::LtEq
        | TokenKind::GtEq => Some((16, 17)),
        // Shift: left-associative
        TokenKind::LtLt | TokenKind::GtGt => Some((18, 19)),
        // Addition/subtraction: left-associative
        TokenKind::Plus | TokenKind::Minus => Some((20, 21)),
        // Multiplication/division/modulo: left-associative
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some((22, 23)),
        _ => None,
    }
}

/// Returns the right_bp for prefix operators.
pub(super) fn prefix_binding_power(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => Some(25),
        _ => None,
    }
}

/// Returns the binding power for assignment operators (right-associative).
pub(super) fn assign_binding_power(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Eq
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::LtLtEq
        | TokenKind::GtGtEq
        | TokenKind::AmpEq
        | TokenKind::PipeEq
        | TokenKind::CaretEq => Some(1),
        _ => None,
    }
}

pub(super) fn token_to_binop(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::Plus => BinOp::Add,
        TokenKind::Minus => BinOp::Sub,
        TokenKind::Star => BinOp::Mul,
        TokenKind::Slash => BinOp::Div,
        TokenKind::Percent => BinOp::Mod,
        TokenKind::LtLt => BinOp::Shl,
        TokenKind::GtGt => BinOp::Shr,
        TokenKind::Amp => BinOp::BitAnd,
        TokenKind::Pipe => BinOp::BitOr,
        TokenKind::Caret => BinOp::BitXor,
        TokenKind::EqEq => BinOp::Eq,
        TokenKind::BangEq => BinOp::Ne,
        TokenKind::Lt => BinOp::Lt,
        TokenKind::LtEq => BinOp::Le,
        TokenKind::Gt => BinOp::Gt,
        TokenKind::GtEq => BinOp::Ge,
        TokenKind::AmpAmp => BinOp::And,
        TokenKind::PipePipe => BinOp::Or,
        _ => unreachable!("not a binop token: {:?}", kind),
    }
}

pub fn parse_int_suffix(text: &str) -> (String, Option<IntSuffix>) {
    const SUFFIXES: &[(&str, IntSuffix)] = &[
        ("i16", IntSuffix::I16),
        ("i32", IntSuffix::I32),
        ("i64", IntSuffix::I64),
        ("i8", IntSuffix::I8),
        ("u16", IntSuffix::U16),
        ("u32", IntSuffix::U32),
        ("u64", IntSuffix::U64),
        ("u8", IntSuffix::U8),
    ];
    for &(s, suffix) in SUFFIXES {
        if let Some(value) = text.strip_suffix(s) {
            // Make sure what remains before the suffix is not empty and ends
            // with a digit or underscore (not another letter).
            if !value.is_empty()
                && value
                    .as_bytes()
                    .last()
                    .is_some_and(|&b| b.is_ascii_digit() || b == b'_')
            {
                return (value.to_string(), Some(suffix));
            }
        }
    }
    (text.to_string(), None)
}

pub fn compound_assign_op(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::PlusEq => BinOp::Add,
        TokenKind::MinusEq => BinOp::Sub,
        TokenKind::StarEq => BinOp::Mul,
        TokenKind::SlashEq => BinOp::Div,
        TokenKind::PercentEq => BinOp::Mod,
        TokenKind::LtLtEq => BinOp::Shl,
        TokenKind::GtGtEq => BinOp::Shr,
        TokenKind::AmpEq => BinOp::BitAnd,
        TokenKind::PipeEq => BinOp::BitOr,
        TokenKind::CaretEq => BinOp::BitXor,
        _ => unreachable!("not a compound assign token: {:?}", kind),
    }
}
