use super::*;
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_core::span::FileId;

fn check_source(source: &str) -> (CheckedModule, DiagnosticBag) {
    let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
    let (module, _) = Parser::new(tokens).parse_module();
    Checker::new().check(&module)
}

#[test]
fn extern_functions_registered() {
    let (checked, diags) = check_source(
        r#"
extern {
fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn main() {}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
    assert!(checked.functions.contains_key("write"));
    let sig = &checked.functions["write"];
    assert_eq!(sig.kind, FunctionKind::Extern);
    assert_eq!(sig.params.len(), 3);
}

#[test]
fn undefined_function_error() {
    let (_, diags) = check_source(
        r#"
fn main() {
foo();
}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("undefined function 'foo'"))
    );
}

#[test]
fn argument_count_mismatch() {
    let (_, diags) = check_source(
        r#"
fn greet(s: string) {}
fn main() {
greet("a", "b");
}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("argument"))
    );
}

#[test]
fn type_mismatch() {
    let (_, diags) = check_source(
        r#"
fn greet(s: string) {}
fn main() {
greet(42);
}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("type mismatch"))
    );
}

#[test]
fn builtins_recognized() {
    let (checked, diags) = check_source(
        r#"
fn main() {
__str_ptr("hello");
__str_len("hello");
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
    assert!(checked.functions.contains_key("__str_ptr"));
    assert!(checked.functions.contains_key("__str_len"));
    assert_eq!(checked.functions["__str_ptr"].kind, FunctionKind::Builtin);
}

#[test]
fn main_validation_preserved() {
    let (_, diags) = check_source("fn foo() {}");
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("no 'main' function"))
    );
}

#[test]
fn user_defined_function_registered() {
    let (checked, diags) = check_source(
        r#"
fn print(s: string) {}
fn main() {
print("hello");
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
    assert!(checked.functions.contains_key("print"));
    let sig = &checked.functions["print"];
    assert_eq!(sig.kind, FunctionKind::UserDefined);
    assert_eq!(sig.params.len(), 1);
}

#[test]
fn duplicate_function_error() {
    let (_, diags) = check_source(
        r#"
fn foo() {}
fn foo() {}
fn main() {}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("duplicate function"))
    );
}

#[test]
fn unknown_type_error() {
    let (_, diags) = check_source(
        r#"
fn foo(x: Blah) {}
fn main() {}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("unknown type"))
    );
}

#[test]
fn target_program_passes() {
    let (checked, diags) = check_source(
        r#"
extern {
fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
write(1, __str_ptr(s), __str_len(s));
}

fn println(s: string) {
print(s);
print("\n");
}

fn main() {
println("Hello, world!");
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
    assert_eq!(checked.functions["write"].kind, FunctionKind::Extern);
    assert_eq!(checked.functions["print"].kind, FunctionKind::UserDefined);
    assert_eq!(checked.functions["println"].kind, FunctionKind::UserDefined);
}

#[test]
fn immutable_assignment_error() {
    let (_, diags) = check_source(
        r#"
fn main() {
let x = 10;
x = 20;
}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("immutable"))
    );
}

#[test]
fn mutable_assignment_ok() {
    let (_, diags) = check_source(
        r#"
fn main() {
let mut x = 10;
x = 20;
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
}

#[test]
fn binary_operator_type_check() {
    let (_, diags) = check_source(
        r#"
fn main() {
let x: i32 = 10;
let y: i32 = 20;
let z = x + y;
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
}

#[test]
fn return_type_check() {
    let (_, diags) = check_source(
        r#"
fn add(a: i32, b: i32) -> i32 {
a + b
}
fn main() {
add(1, 2);
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
}

#[test]
fn return_type_mismatch() {
    let (_, diags) = check_source(
        r#"
fn foo() -> i32 {
true
}
fn main() {}
"#,
    );
    assert!(diags.has_errors());
    assert!(
        diags
            .reports()
            .iter()
            .any(|r| r.message.contains("return type"))
    );
}

#[test]
fn if_condition_must_be_bool() {
    let (_, diags) = check_source(
        r#"
fn main() {
if 42 {}
}
"#,
    );
    assert!(diags.has_errors());
}

#[test]
fn while_with_comparison() {
    let (_, diags) = check_source(
        r#"
fn main() {
let mut x: i32 = 0;
while x < 10 {
    x = x + 1;
}
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
}

#[test]
fn target_program_v2_passes() {
    let (_, diags) = check_source(
        r#"
extern {
fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
write(1, __str_ptr(s), __str_len(s));
}

fn println(s: string) {
print(s);
print("\n");
}

fn add(a: i32, b: i32) -> i32 {
a + b
}

fn main() {
let x: i32 = 10;
let y = 20;
let sum = add(x, y);

if sum > 25 {
    println("big");
} else {
    println("small");
}

let mut counter: i32 = 0;
while counter < 10 {
    counter = counter + 1;
}
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );
}

#[test]
fn block_scoping_hides_inner_variables() {
    let (_, diags) = check_source(
        r#"
fn main() {
let n = {
    let z = 42;
    z
};
n;
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "unexpected errors: {:?}",
        diags.reports()
    );

    // z should NOT be accessible outside the block
    let (_, diags) = check_source(
        r#"
fn id(x: i32) -> i32 { x }
fn main() {
let n = {
    let z = 42;
    z
};
id(z);
}
"#,
    );
    assert!(
        diags.has_errors(),
        "z should be undefined outside the block"
    );
}

#[test]
fn if_scoping_hides_inner_variables() {
    let (_, diags) = check_source(
        r#"
fn id(x: i32) -> i32 { x }
fn main() {
if true {
    let y = 10;
}
id(y);
}
"#,
    );
    assert!(
        diags.has_errors(),
        "y should be undefined outside the if block"
    );
}

#[test]
fn while_scoping_hides_inner_variables() {
    let (_, diags) = check_source(
        r#"
fn id(x: i32) -> i32 { x }
fn main() {
let mut x: i32 = 0;
while x < 1 {
    let inner = 5;
    x = x + 1;
}
id(inner);
}
"#,
    );
    assert!(
        diags.has_errors(),
        "inner should be undefined outside the while block"
    );
}

// --- Phase 3: Named arguments ---

#[test]
fn named_args_type_check() {
    let (_, diags) = check_source(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() {
let r = add(1, b: 2);
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "named args should type check: {:?}",
        diags.reports()
    );
}

#[test]
fn named_args_wrong_name() {
    let (_, diags) = check_source(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() {
let r = add(1, c: 2);
}
"#,
    );
    assert!(diags.has_errors(), "unknown named arg 'c' should fail");
}

#[test]
fn default_params_type_check() {
    let (_, diags) = check_source(
        r#"
fn greet(name: string, times: i32 = 1) -> i32 { times }
fn main() {
let a = greet("hello");
let b = greet("world", times: 3);
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "default params should type check: {:?}",
        diags.reports()
    );
}

#[test]
fn default_params_missing_required() {
    let (_, diags) = check_source(
        r#"
fn greet(name: string, times: i32 = 1) -> i32 { times }
fn main() {
let a = greet();
}
"#,
    );
    assert!(diags.has_errors(), "missing required arg should fail");
}

// --- Phase 3: Impl blocks ---

#[test]
fn impl_block_static_method_check() {
    let (checked, diags) = check_source(
        r#"
struct Point { x: i32, y: i32 }
impl Point {
fn new(x: i32, y: i32) -> Point {
    Point { x: x, y: y }
}
}
fn main() {
let p = Point::new(3, y: 4);
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "static method should type check: {:?}",
        diags.reports()
    );
    assert!(
        checked.functions.contains_key("Point__new"),
        "mangled name should be registered"
    );
}

#[test]
fn impl_block_instance_method_check() {
    let (_, diags) = check_source(
        r#"
struct Counter { value: i32 }
impl Counter {
fn new(start: i32) -> Counter { Counter { value: start } }
fn get(self) -> i32 { self.value }
fn increment(mut self) { self.value = self.value + 1; }
}
fn main() {
let mut c = Counter::new(0);
c.increment();
let v = c.get();
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "instance methods should type check: {:?}",
        diags.reports()
    );
}

#[test]
fn impl_block_undefined_method() {
    let (_, diags) = check_source(
        r#"
struct Point { x: i32, y: i32 }
fn main() {
let p = Point { x: 1, y: 2 };
p.nonexistent();
}
"#,
    );
    assert!(diags.has_errors(), "calling undefined method should fail");
}

#[test]
fn struct_field_shorthand_check() {
    let (_, diags) = check_source(
        r#"
struct Point { x: i32, y: i32 }
fn main() {
let x = 3;
let y = 4;
let p = Point { x, y };
}
"#,
    );
    assert!(
        !diags.has_errors(),
        "struct field shorthand should type check: {:?}",
        diags.reports()
    );
}
