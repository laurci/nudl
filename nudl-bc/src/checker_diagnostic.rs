use meta::Diagnostic;
use nudl_core::span::Span;

#[derive(Diagnostic)]
#[section(Checker)]
pub enum CheckerDiagnostic {
    #[message("undefined function '{name}'")]
    #[severity(Error)]
    UndefinedFunction { span: Span, name: String },

    #[message("expected {expected} argument(s), found {found}")]
    #[severity(Error)]
    ArgumentCountMismatch {
        span: Span,
        expected: String,
        found: String,
    },

    #[message("type mismatch: expected '{expected}', found '{found}'")]
    #[severity(Error)]
    TypeMismatch {
        span: Span,
        expected: String,
        found: String,
    },

    #[message("'main' function must take no parameters and return ()")]
    #[severity(Error)]
    InvalidMainSignature { span: Span },

    #[message("no 'main' function found")]
    #[severity(Error)]
    NoMainFunction { span: Span },

    #[message("unknown type '{name}'")]
    #[severity(Error)]
    UnknownType { span: Span, name: String },

    #[message("duplicate function '{name}'")]
    #[severity(Error)]
    DuplicateFunction { span: Span, name: String },

    #[message("undefined variable '{name}'")]
    #[severity(Error)]
    UndefinedVariable { span: Span, name: String },

    #[message("cannot assign to immutable variable '{name}'")]
    #[severity(Error)]
    ImmutableAssignment { span: Span, name: String },

    #[message("operator '{op}' cannot be applied to type '{ty}'")]
    #[severity(Error)]
    InvalidOperatorType {
        span: Span,
        op: String,
        ty: String,
    },

    #[message("expected return type '{expected}', found '{found}'")]
    #[severity(Error)]
    ReturnTypeMismatch {
        span: Span,
        expected: String,
        found: String,
    },
}
