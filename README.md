# nudl

A high-level, statically-typed, reference-counted, compiled programming language.

> The power and syntax of Rust, with the memory management of Swift and the metaprogramming of Zig.

## At a Glance

```nudl
fn main() {
    let sum = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        |> filter { it % 2 == 0 }
        |> map { it * 2 }
        |> fold(initial: 0) { acc, item -> acc + item };

    println(`Sum of doubled evens: {sum}`);  // Sum of doubled evens: 60
}
```

## Features

- **Rust-grade expressiveness** -- Algebraic data types, pattern matching with exhaustiveness checking, generics with monomorphization, interfaces for bounded polymorphism
- **Swift-grade memory management** -- Automatic Reference Counting with compiler-inserted retain/release. No borrow checker, no lifetimes, no garbage collector
- **Zig-grade metaprogramming** -- `comptime` blocks execute at compile time in a sandboxed VM. Types are first-class values. Code generation via `quote` blocks
- **Practical ergonomics** -- Named arguments, trailing lambdas with implicit `it`, string interpolation, defer, spread operators

```nudl
// Comptime code generation
comptime fn make_vector_type(comptime n: u32) {
    let names = ["x", "y", "z", "w"];
    quote {
        struct ${`Vec{n}`} {
            ${for name in names[0..n] { quote { ${name}: f64, } }}
        }
    }
}

comptime {
    make_vector_type(2);  // generates Vec2 { x: f64, y: f64 }
    make_vector_type(3);  // generates Vec3 { x: f64, y: f64, z: f64 }
}
```

```nudl
// ARC-managed linked list
enum List<T> {
    Cons { head: T, tail: List<T> },
    Nil,
}

impl<T: Printable> List<T> {
    fn prepend(self, value: T) -> List<T> {
        List::Cons { head: value, tail: self }
    }
}

fn main() {
    let list = List::new().prepend(3).prepend(2).prepend(1);
    println(`Length: {list.len()}`);
}
```

## Non-Goals

- **No borrow checker.** Memory safety comes from ARC, not static lifetime analysis
- **No garbage collector.** Memory is reclaimed deterministically when the last reference drops
- **No exceptions.** Error handling uses `Result<T, E>` and `Option<T>` with `?`
- **No null.** Optional values are `Option<T>`
- **No inheritance.** Polymorphism comes from interfaces and generics

## Building

```bash
cargo build --workspace
cargo test --workspace
```

## Project Structure

The compiler is organized as a Cargo workspace with 9 crates, following the compilation pipeline:

```
Source (.nudl)
  → nudl-ast        Lex and parse into an untyped AST
  → nudl-bc         Type-check, infer, monomorphize, lower to SSA bytecode
  → nudl-vm         Execute comptime blocks in a sandboxed VM
  → nudl-backend-arm64   Compile SSA bytecode to ARM64 machine code
  → nudl-packer-macho    Pack into a Mach-O executable (macOS)
  → nudl-packer-elf      Pack into an ELF executable (Linux)
```

`nudl-core` provides shared foundations (spans, diagnostics, type representations). `nudl-cli` and `nudl-lsp` are the user-facing frontends.

## Documentation

- **[Feature Overview](docs/features/README.md)** -- Design philosophy, feature matrix, language comparisons, and example programs
- **[Language Specification](docs/spec/README.md)** -- Normative spec covering lexical structure, type system, memory model, expressions, pattern matching, comptime, and full grammar
- **[Compiler Internals](docs/internals/README.md)** -- Architecture, SSA bytecode design, VM execution model, ARC implementation, native backend, and binary packers

## Status

nudl is in the design phase. The language specification is complete and the compiler crates are scaffolded, but implementation has not started.
