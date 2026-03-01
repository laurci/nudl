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
    InvalidOperatorType { span: Span, op: String, ty: String },

    #[message("expected return type '{expected}', found '{found}'")]
    #[severity(Error)]
    ReturnTypeMismatch {
        span: Span,
        expected: String,
        found: String,
    },

    #[message("undefined struct '{name}'")]
    #[severity(Error)]
    UndefinedStruct { span: Span, name: String },

    #[message("missing field '{field}' in struct '{name}'")]
    #[severity(Error)]
    MissingField {
        span: Span,
        name: String,
        field: String,
    },

    #[message("unknown field '{field}' on struct '{name}'")]
    #[severity(Error)]
    UnknownField {
        span: Span,
        name: String,
        field: String,
    },

    #[message("duplicate struct '{name}'")]
    #[severity(Error)]
    DuplicateStruct { span: Span, name: String },

    #[message("field access on non-struct type '{ty}'")]
    #[severity(Error)]
    FieldAccessOnNonStruct { span: Span, ty: String },

    #[message("unknown parameter name '{name}'")]
    #[severity(Error)]
    UnknownParameterName { span: Span, name: String },

    #[message("missing required argument '{name}'")]
    #[severity(Error)]
    MissingRequiredArgument { span: Span, name: String },

    #[message("no method '{method}' on type '{ty}'")]
    #[severity(Error)]
    UndefinedMethod {
        span: Span,
        ty: String,
        method: String,
    },

    #[message("cannot call mutating method '{method}' on immutable binding")]
    #[severity(Error)]
    MutatingMethodOnImmutable { span: Span, method: String },

    #[message("cannot infer type for type parameter '{name}'")]
    #[severity(Error)]
    CannotInferTypeParam { span: Span, name: String },

    #[message("expected {expected} type argument(s), found {found}")]
    #[severity(Error)]
    TypeArgCountMismatch {
        span: Span,
        expected: usize,
        found: usize,
    },

    #[message("bound '{interface}' not satisfied for type '{ty}'")]
    #[severity(Error)]
    BoundCheckFailed {
        span: Span,
        ty: String,
        interface: String,
    },

    #[message("operation on type parameter '{type_param}' requires '{bound}' bound")]
    #[severity(Error)]
    BoundNotSatisfied {
        span: Span,
        type_param: String,
        bound: String,
    },

    #[message("impl {interface_name} for {type_name} is missing method '{method}'")]
    #[severity(Error)]
    MissingInterfaceMethod {
        span: Span,
        type_name: String,
        interface_name: String,
        method: String,
    },

    #[message(
        "method '{method}' on {type_name} has wrong signature for {interface_name}: expected {expected}, found {found}"
    )]
    #[severity(Error)]
    InterfaceMethodSignatureMismatch {
        span: Span,
        type_name: String,
        interface_name: String,
        method: String,
        expected: String,
        found: String,
    },

    #[message("function '{name}' is not public")]
    #[severity(Error)]
    PrivateFunction { span: Span, name: String },

    #[message("method '{method}' on type '{ty}' is not public")]
    #[severity(Error)]
    PrivateMethod {
        span: Span,
        ty: String,
        method: String,
    },

    #[message("field '{field}' on struct '{name}' is not public")]
    #[severity(Error)]
    PrivateField {
        span: Span,
        name: String,
        field: String,
    },

    #[message("type '{name}' is not public")]
    #[severity(Error)]
    PrivateType { span: Span, name: String },
}
