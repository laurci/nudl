use meta::Diagnostic;
use nudl_core::span::Span;

#[derive(Diagnostic)]
#[section(Parser)]
pub enum ParserDiagnostic {
    #[message("unexpected token: expected {expected}, found '{found}'")]
    #[severity(Error)]
    UnexpectedToken { span: Span, expected: String, found: String },

    #[message("unexpected end of file: expected {expected}")]
    #[severity(Error)]
    UnexpectedEof { span: Span, expected: String },

    #[message("expected expression")]
    #[severity(Error)]
    ExpectedExpression { span: Span },

    #[message("expected type")]
    #[severity(Error)]
    ExpectedType { span: Span },
}
