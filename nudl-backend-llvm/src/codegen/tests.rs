#[cfg(test)]
mod tests {
    use crate::codegen::{compile_to_asm_text, compile_to_executable, compile_to_llvm_ir};
    use nudl_bc::ir::Program;
    use std::process::Command;

    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_core::span::FileId;

    fn compile_source(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        Lowerer::new(checked).lower(&module)
    }

    fn compile_and_run(source: &str) -> (String, bool) {
        let program = compile_source(source);
        let output = std::env::temp_dir().join("nudl_llvm_test");
        compile_to_executable(&program, &output, false, false, &[]).expect("compilation failed");

        assert!(output.exists(), "output binary should exist");

        let result = Command::new(&output)
            .output()
            .expect("failed to run binary");

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let success = result.status.success();

        let _ = std::fs::remove_file(&output);

        (stdout, success)
    }

    #[test]
    fn compile_hello_world() {
        let (stdout, success) = compile_and_run(
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

        assert_eq!(stdout, "Hello, world!\n");
        assert!(success, "binary should exit with 0");
    }

    #[test]
    fn compile_with_arithmetic() {
        let program = compile_source(
            r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
fn main() {
    let x = add(10, 20);
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("add"));
    }

    #[test]
    fn compile_with_if_else() {
        let program = compile_source(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}

fn main() {
    let x = 1;
    if x == 1 {
        print("yes\n");
    } else {
        print("no\n");
    }
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("br i1"));
    }

    #[test]
    fn emit_llvm_ir() {
        let program = compile_source(
            r#"
fn main() {
    let x = 42;
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
    }

    #[test]
    fn emit_asm() {
        let program = compile_source(
            r#"
fn main() {
    let x = 42;
}
"#,
        );
        let asm = compile_to_asm_text(&program, false, false).expect("ASM generation failed");
        assert!(!asm.is_empty());
        assert!(asm.contains("main"));
    }

    #[test]
    fn compile_extern_struct() {
        let program = compile_source(
            r#"
extern struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn main() {
    let c = Color { r: 255u8, g: 0u8, b: 0u8, a: 255u8 };
    let r = c.r;
    let g = c.g;
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
        // Extern struct should NOT have a drop function
        assert!(!ir.contains("__nudl_drop_Color"));
    }

    #[test]
    fn compile_extern_struct_as_extern_param() {
        let program = compile_source(
            r#"
extern struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

extern {
    fn SomeFunction(color: Color);
}

fn main() {
    let c = Color { r: 255u8, g: 128u8, b: 0u8, a: 255u8 };
    SomeFunction(c);
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        // The extern function should declare a struct param type, not i64
        assert!(ir.contains("{ i8, i8, i8, i8 }"));
    }

    #[test]
    fn compile_cptr_builtin() {
        let program = compile_source(
            r#"
extern struct Point {
    x: i32,
    y: i32,
}

extern {
    fn SomeApi(ptr: RawPtr);
}

fn main() {
    let v = Point { x: 1, y: 2 };
    SomeApi(cptr(v));
}
"#,
        );
        let ir = compile_to_llvm_ir(&program).expect("IR generation failed");
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("cptr_struct"));
    }
}
