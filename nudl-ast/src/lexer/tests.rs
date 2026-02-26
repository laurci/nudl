use super::*;

fn lex(source: &str) -> Vec<Token> {
    let (tokens, diags) = Lexer::new(source, FileId(0)).tokenize();
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
    tokens
}

fn lex_kinds(source: &str) -> Vec<TokenKind> {
    lex(source).into_iter().map(|t| t.kind).collect()
}

#[test]
fn hello_world() {
    let tokens = lex(r#"fn main() {
    println("Hello, world!");
}"#);
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Fn,
            TokenKind::Ident, // main
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::Ident, // println
            TokenKind::LParen,
            TokenKind::StringLiteral,
            TokenKind::RParen,
            TokenKind::Semi,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
    // Verify string literal value
    let str_tok = tokens
        .iter()
        .find(|t| t.kind == TokenKind::StringLiteral)
        .unwrap();
    assert_eq!(str_tok.text, "Hello, world!");
}

#[test]
fn string_escapes() {
    let tokens = lex(r#""hello\nworld\t\\""#);
    let str_tok = &tokens[0];
    assert_eq!(str_tok.kind, TokenKind::StringLiteral);
    assert_eq!(str_tok.text, "hello\nworld\t\\");
}

#[test]
fn nested_comments() {
    let kinds = lex_kinds("/* outer /* inner */ */ 42");
    assert_eq!(kinds, vec![TokenKind::IntLiteral, TokenKind::Eof]);
}

#[test]
fn operators() {
    let kinds = lex_kinds("-> => == != <= >= && || << >> :: ..");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Arrow,
            TokenKind::FatArrow,
            TokenKind::EqEq,
            TokenKind::BangEq,
            TokenKind::LtEq,
            TokenKind::GtEq,
            TokenKind::AmpAmp,
            TokenKind::PipePipe,
            TokenKind::LtLt,
            TokenKind::GtGt,
            TokenKind::ColonColon,
            TokenKind::DotDot,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn numbers() {
    let tokens = lex("42 0xff 0b1010 3.14 1e10");
    assert_eq!(tokens[0].kind, TokenKind::IntLiteral);
    assert_eq!(tokens[0].text, "42");
    assert_eq!(tokens[1].kind, TokenKind::IntLiteral);
    assert_eq!(tokens[1].text, "0xff");
    assert_eq!(tokens[2].kind, TokenKind::IntLiteral);
    assert_eq!(tokens[2].text, "0b1010");
    assert_eq!(tokens[3].kind, TokenKind::FloatLiteral);
    assert_eq!(tokens[3].text, "3.14");
    assert_eq!(tokens[4].kind, TokenKind::FloatLiteral);
    assert_eq!(tokens[4].text, "1e10");
}

#[test]
fn error_unterminated_string() {
    let (tokens, diags) = Lexer::new(r#""hello"#, FileId(0)).tokenize();
    assert!(diags.has_errors());
    assert_eq!(tokens[0].kind, TokenKind::Error);
}

#[test]
fn error_unexpected_char() {
    let (_, diags) = Lexer::new("\u{00a7}", FileId(0)).tokenize();
    assert!(diags.has_errors());
}

#[test]
fn template_string_no_interpolation() {
    let tokens = lex("`hello, world`");
    assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
    assert_eq!(tokens[0].text, "hello, world");
}

#[test]
fn template_string_single_interpolation() {
    let tokens = lex("`hello, {name}!`");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::TemplateStringStart,
            TokenKind::Ident,
            TokenKind::TemplateStringEnd,
            TokenKind::Eof,
        ]
    );
    assert_eq!(tokens[0].text, "hello, ");
    assert_eq!(tokens[1].text, "name");
    assert_eq!(tokens[2].text, "!");
}

#[test]
fn template_string_multiple_interpolations() {
    let tokens = lex("`{a} + {b} = {c}`");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::TemplateStringStart,
            TokenKind::Ident, // a
            TokenKind::TemplateStringPart,
            TokenKind::Ident, // b
            TokenKind::TemplateStringPart,
            TokenKind::Ident, // c
            TokenKind::TemplateStringEnd,
            TokenKind::Eof,
        ]
    );
    assert_eq!(tokens[0].text, "");
    assert_eq!(tokens[2].text, " + ");
    assert_eq!(tokens[4].text, " = ");
    assert_eq!(tokens[6].text, "");
}

#[test]
fn template_string_with_braces_in_expr() {
    // Expression contains braces (e.g., a block or struct literal)
    let tokens = lex("`result: {if true { 1 } else { 2 }}`");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::TemplateStringStart, // "result: "
            TokenKind::If,
            TokenKind::True,
            TokenKind::LBrace,
            TokenKind::IntLiteral, // 1
            TokenKind::RBrace,
            TokenKind::Else,
            TokenKind::LBrace,
            TokenKind::IntLiteral, // 2
            TokenKind::RBrace,
            TokenKind::TemplateStringEnd, // ""
            TokenKind::Eof,
        ]
    );
}

#[test]
fn template_string_escapes() {
    let tokens = lex(r"`\{not interpolated\}`");
    assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
    assert_eq!(tokens[0].text, "{not interpolated}");
}

#[test]
fn template_string_backtick_escape() {
    let tokens = lex(r"`contains a \` backtick`");
    assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
    assert_eq!(tokens[0].text, "contains a ` backtick");
}

#[test]
fn error_unterminated_template_string() {
    let (tokens, diags) = Lexer::new("`hello", FileId(0)).tokenize();
    assert!(diags.has_errors());
    assert_eq!(tokens[0].kind, TokenKind::Error);
}
