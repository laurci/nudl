use super::*;
use crate::checker::Checker;
use nudl_ast::lexer::Lexer;
use nudl_ast::parser::Parser;
use nudl_core::span::FileId;

fn lower_source(source: &str) -> Program {
    let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
    let (module, _) = Parser::new(tokens).parse_module();
    let (checked, diags) = Checker::new().check(&module);
    assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
    Lowerer::new(checked).lower(&module)
}

#[test]
fn lower_target_program() {
    let program = lower_source(
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

    // 4 functions: write (extern), print, println, main
    assert_eq!(
        program.functions.len(),
        4,
        "expected 4 functions, got {}",
        program.functions.len()
    );

    // write should be extern
    let write_fn = &program.functions[0];
    assert!(write_fn.is_extern);
    assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));

    // String constants should include "Hello, world!" and "\n"
    assert!(
        program
            .string_constants
            .contains(&"Hello, world!".to_string()),
        "missing 'Hello, world!' in {:?}",
        program.string_constants
    );
    assert!(
        program.string_constants.contains(&"\n".to_string()),
        "missing '\\n' in {:?}",
        program.string_constants
    );

    // Entry function should be main
    assert!(program.entry_function.is_some());

    // print function should have StringPtr and StringLen instructions
    let print_fn = &program.functions[1];
    assert!(!print_fn.is_extern);
    assert_eq!(print_fn.params.len(), 1);
    let has_str_ptr = print_fn.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|i| matches!(i, Instruction::StringPtr(_, _)))
    });
    let has_str_len = print_fn.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|i| matches!(i, Instruction::StringLen(_, _)))
    });
    assert!(has_str_ptr, "print should have StringPtr instruction");
    assert!(has_str_len, "print should have StringLen instruction");
}

#[test]
fn lower_has_return() {
    let program = lower_source(
        r#"
fn main() {
    __str_ptr("hi");
}
"#,
    );
    let main_func = program.functions.iter().find(|f| !f.is_extern).unwrap();
    let last_block = main_func.blocks.last().unwrap();
    assert!(matches!(last_block.terminator, Terminator::Return(_)));
}

#[test]
fn extern_function_lowered() {
    let program = lower_source(
        r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn main() {}
"#,
    );
    let write_fn = program.functions.iter().find(|f| f.is_extern).unwrap();
    assert!(write_fn.blocks.is_empty());
    assert_eq!(write_fn.extern_symbol.as_deref(), Some("write"));
}

#[test]
fn params_assigned_to_registers() {
    let program = lower_source(
        r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#,
    );
    let greet_fn = &program.functions[0];
    assert_eq!(greet_fn.params.len(), 1);
}

#[test]
fn string_dedup() {
    let program = lower_source(
        r#"
fn main() {
    __str_ptr("same");
    __str_ptr("same");
}
"#,
    );
    // "same" should appear only once
    assert_eq!(
        program
            .string_constants
            .iter()
            .filter(|s| *s == "same")
            .count(),
        1
    );
}

#[test]
fn lower_binary_ops() {
    let program = lower_source(
        r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    add(1, 2);
}
"#,
    );
    let add_fn = &program.functions[0];
    let has_add = add_fn.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|i| matches!(i, Instruction::Add(_, _, _)))
    });
    assert!(has_add, "add function should have Add instruction");
}

#[test]
fn lower_if_creates_blocks() {
    let program = lower_source(
        r#"
fn main() {
    let x: i32 = 10;
    if x > 5 {
        __str_ptr("yes");
    } else {
        __str_ptr("no");
    }
}
"#,
    );
    let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
    // If/else should create multiple blocks
    assert!(
        main_fn.blocks.len() >= 4,
        "expected at least 4 blocks for if/else, got {}",
        main_fn.blocks.len()
    );
}

#[test]
fn lower_while_creates_blocks() {
    let program = lower_source(
        r#"
fn main() {
    let mut x: i32 = 0;
    while x < 10 {
        x = x + 1;
    }
}
"#,
    );
    let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
    // While should create multiple blocks
    assert!(
        main_fn.blocks.len() >= 3,
        "expected at least 3 blocks for while, got {}",
        main_fn.blocks.len()
    );
}

#[test]
fn lower_target_program_v2() {
    let program = lower_source(
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
    assert!(program.entry_function.is_some());
    assert!(
        program.functions.len() >= 5,
        "expected at least 5 functions (write, print, println, add, main)"
    );
}

#[test]
fn lower_struct_alloc_store_load() {
    let program = lower_source(
        r#"
struct Point { x: i32, y: i32 }
fn main() {
    let p = Point { x: 42, y: 17 };
    let val = p.x;
}
"#,
    );
    let main_fn = program.functions.iter().find(|f| !f.is_extern).unwrap();
    let all_insts: Vec<&Instruction> = main_fn
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .collect();

    assert!(
        all_insts
            .iter()
            .any(|i| matches!(i, Instruction::Alloc(_, _))),
        "expected Alloc instruction for struct literal"
    );
    assert!(
        all_insts
            .iter()
            .any(|i| matches!(i, Instruction::Store(_, _, _))),
        "expected Store instruction for field init"
    );
    assert!(
        all_insts
            .iter()
            .any(|i| matches!(i, Instruction::Load(_, _, _))),
        "expected Load instruction for field access"
    );
    assert!(
        all_insts
            .iter()
            .any(|i| matches!(i, Instruction::Release(_, _))),
        "expected Release instruction for scope exit"
    );
}

#[test]
fn lower_struct_caller_retain_callee_release() {
    let program = lower_source(
        r#"
struct Point { x: i32, y: i32 }
fn use_point(p: Point) {
    let val = p.x;
}
fn main() {
    let p = Point { x: 1, y: 2 };
    use_point(p);
}
"#,
    );
    // Check main has Retain (caller-retain before calling use_point)
    let main_fn = program
        .functions
        .iter()
        .find(|f| {
            let name = program.interner.resolve(f.name);
            name == "main"
        })
        .unwrap();
    let main_insts: Vec<&Instruction> = main_fn
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .collect();
    assert!(
        main_insts
            .iter()
            .any(|i| matches!(i, Instruction::Retain(_))),
        "expected Retain in main (caller-retain)"
    );

    // Check use_point has Release (callee-release of param)
    let use_fn = program
        .functions
        .iter()
        .find(|f| {
            let name = program.interner.resolve(f.name);
            name == "use_point"
        })
        .unwrap();
    let use_insts: Vec<&Instruction> = use_fn
        .blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .collect();
    assert!(
        use_insts
            .iter()
            .any(|i| matches!(i, Instruction::Release(_, _))),
        "expected Release in use_point (callee-release)"
    );
}

// --- Phase 3: Named arguments ---

#[test]
fn named_arguments_basic() {
    let program = lower_source(
        r#"
fn power(base: i32, exponent: i32) -> i32 {
    base * exponent
}

fn main() {
    let r = power(2, exponent: 3);
}
"#,
    );
    assert!(program.entry_function.is_some());
}

#[test]
fn named_arguments_all_named() {
    let program = lower_source(
        r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let r = add(a: 1, b: 2);
}
"#,
    );
    assert!(program.entry_function.is_some());
}

// --- Phase 3: Default parameters ---

#[test]
fn default_params_basic() {
    let program = lower_source(
        r#"
fn repeat_string(s: string, times: i32 = 3) -> i32 {
    times
}

fn main() {
    let a = repeat_string("hello");
    let b = repeat_string("world", times: 5);
}
"#,
    );
    assert!(program.entry_function.is_some());
}

#[test]
fn default_params_multiple() {
    let program = lower_source(
        r#"
fn connect(host: string, port: i32 = 8080, timeout_ms: i32 = 30000) -> i32 {
    port
}

fn main() {
    let a = connect("localhost");
    let b = connect("localhost", port: 9090);
    let c = connect("localhost", port: 9090, timeout_ms: 5000);
}
"#,
    );
    assert!(program.entry_function.is_some());
}

// --- Phase 3: Impl blocks and methods ---

#[test]
fn impl_block_static_method() {
    let program = lower_source(
        r#"
struct Point {
    x: i32,
    y: i32,
}

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
    // Should have a function named Point__new
    let has_point_new = program
        .functions
        .iter()
        .any(|f| program.interner.resolve(f.name) == "Point__new");
    assert!(has_point_new, "expected Point__new function");
}

#[test]
fn impl_block_instance_method() {
    let program = lower_source(
        r#"
struct Counter {
    value: i32,
}

impl Counter {
    fn new(start: i32) -> Counter {
        Counter { value: start }
    }

    fn get(self) -> i32 {
        self.value
    }

    fn increment(mut self) {
        self.value = self.value + 1;
    }
}

fn main() {
    let mut c = Counter::new(0);
    c.increment();
    let v = c.get();
}
"#,
    );
    let has_counter_get = program
        .functions
        .iter()
        .any(|f| program.interner.resolve(f.name) == "Counter__get");
    let has_counter_increment = program
        .functions
        .iter()
        .any(|f| program.interner.resolve(f.name) == "Counter__increment");
    assert!(has_counter_get, "expected Counter__get function");
    assert!(
        has_counter_increment,
        "expected Counter__increment function"
    );
}

// --- Phase 3: Struct field shorthand ---

#[test]
fn struct_field_shorthand() {
    let program = lower_source(
        r#"
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let x = 3;
    let y = 4;
    let p = Point { x, y };
}
"#,
    );
    assert!(program.entry_function.is_some());
}
