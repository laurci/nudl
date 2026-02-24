use meta::Diagnostic;
use nudl_core::span::Span;

#[derive(Diagnostic)]
#[section(Lexer)]
pub enum LexerDiagnostic {
    #[message("unexpected character '{ch}'")]
    #[severity(Error)]
    UnexpectedChar { span: Span, ch: char },

    #[message("unterminated string literal")]
    #[severity(Error)]
    UnterminatedString { span: Span },

    #[message("unterminated template string literal")]
    #[severity(Error)]
    UnterminatedTemplateString { span: Span },

    #[message("unterminated block comment")]
    #[severity(Error)]
    UnterminatedBlockComment { span: Span },

    #[message("invalid escape sequence '\\{ch}'")]
    #[severity(Error)]
    InvalidEscape { span: Span, ch: char },
}
