use crate::*;

use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_bc::checker::Checker;
use nudl_bc::ir::Program;
use nudl_bc::lower::Lowerer;
use nudl_core::span::FileId;

fn compile(source: &str) -> Program {
    let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
    let (module, _) = Parser::new(tokens).parse_module();
    let (checked, diags) = Checker::new().check(&module);
    assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
    Lowerer::new(checked).lower(&module)
}

#[test]
fn run_empty_main() {
    let program = compile("fn main() {}");
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Unit));
}

#[test]
fn run_function_call() {
    let program = compile(
        r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn run_nested_calls() {
    let program = compile(
        r#"
fn inner(s: string) {}
fn outer(s: string) {
    inner(s);
}
fn main() {
    outer("hello");
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn extern_call_fails() {
    let program = compile(
        r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}
fn main() {
    print("hello");
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_err());
    match result.unwrap_err() {
        VmError::ExternCallNotAllowed { function_name } => {
            assert_eq!(function_name, "write");
        }
        other => panic!("expected ExternCallNotAllowed, got {:?}", other),
    }
}

#[test]
fn step_limit_exceeded() {
    let program = compile(
        r#"
fn a(s: string) {}
fn b(s: string) { a(s); }
fn c(s: string) { b(s); }
fn d(s: string) { c(s); }
fn main() {
    d("x");
}
"#,
    );
    let mut vm = Vm::with_step_limit(5);
    let result = vm.run(&program);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        VmError::StepLimitExceeded { .. }
    ));
}

#[test]
fn string_builtins_work() {
    let program = compile(
        r#"
fn main() {
    __str_ptr("hello");
    __str_len("hello");
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn no_entry_function_error() {
    let program = Program {
        functions: vec![],
        string_constants: vec![],
        entry_function: None,
        extern_libs: vec![],
        interner: nudl_core::intern::StringInterner::new(),
        types: nudl_core::types::TypeInterner::new(),
        source_map: None,
    };
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), VmError::NoEntryFunction));
}

#[test]
fn vm_arithmetic() {
    let program = compile(
        r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    let result = add(10, 20);
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn vm_if_else() {
    let program = compile(
        r#"
fn pick(x: i32) -> i32 {
    if x > 5 { 1 } else { 0 }
}
fn main() {
    let a = pick(10);
    let b = pick(3);
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn vm_while_loop() {
    let program = compile(
        r#"
fn main() {
    let mut x: i32 = 0;
    while x < 10 {
        x = x + 1;
    }
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn vm_loop_break() {
    let program = compile(
        r#"
fn main() {
    let mut x: i32 = 0;
    loop {
        x = x + 1;
        if x > 5 {
            break;
        }
    }
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}

#[test]
fn vm_function_return_value() {
    let program = compile(
        r#"
fn double(x: i32) -> i32 {
    x + x
}
fn main() {
    let a = double(21);
}
"#,
    );
    let mut vm = Vm::new();
    let result = vm.run(&program);
    assert!(result.is_ok());
}
