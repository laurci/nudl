# nudl -- Feature Overview

## 1. Overview and Philosophy

### Vision

> The power and syntax of Rust, with the memory management of Swift and the metaprogramming of Zig.

nudl is a high-level, statically-typed, reference-counted, compiled programming language. It
borrows the expressive syntax and strong type system from Rust, adopts automatic reference counting
from Swift for predictable memory management without a borrow checker, and takes compile-time
evaluation from Zig to enable powerful metaprogramming without procedural macros.

### Design Goals

- **Rust-grade expressiveness.** Algebraic data types, pattern matching with exhaustiveness
  checking, expression-based control flow, generics with monomorphization, and interfaces for
  bounded polymorphism.
- **Swift-grade memory management.** Automatic Reference Counting (ARC) with compiler-inserted
  retain/release. No borrow checker, no lifetimes, no garbage collector.
- **Zig-grade metaprogramming.** `comptime` blocks and functions that execute at compile time in a
  sandboxed VM. Types are first-class values at comptime. Code generation produces functions,
  structs, and impl blocks without AST-level macro complexity.
- **Practical ergonomics.** Named arguments, trailing lambdas with implicit `it`, string
  interpolation, defer, spread operators, and a clean module system.

### Non-Goals

- **No borrow checker.** Memory safety comes from ARC, not static lifetime analysis.
- **No garbage collector.** Memory is reclaimed deterministically when the last reference drops.
- **No exceptions.** Error handling uses `Result<T, E>` and `Option<T>` with `?`.
- **No null.** Optional values are represented by `Option<T>`.
- **No inheritance.** Polymorphism comes from interfaces and generics.

---

## 2. Core Feature Matrix

| Feature | Description | Status |
|---|---|---|
| Static typing | Hindley-Milner-style bidirectional type inference | Planned |
| Generics | Monomorphized, bounded by interfaces: `fn f<T: Bound>(x: T)` | Planned |
| Interfaces | Bounded polymorphism (no associated types) | Planned |
| ARC | Compiler-inserted retain/release for reference types | Planned |
| Weak references | `let weak r = x;` modifier syntax (internal `WeakRef<T>` never user-written) | Planned |
| Pattern matching | Exhaustive `match` with guards, nested patterns, or-patterns | Planned |
| Result/Option | Built-in enums with `?` operator | Planned |
| Closures | `\|params\| expr` with ARC-captured environment | Planned |
| Trailing lambdas | `f(args) { body }` with implicit `it` for single-param | Planned |
| Named arguments | First non-self param positional, rest named | Planned |
| Default params | `fn connect(host: string, port: u16 = 8080)` | Planned |
| Optional params | `fn test(arg1?: Type)` desugars to `Option<Type>` | Planned |
| Defer | `defer { cleanup(); }` -- LIFO, always runs on scope exit | Planned |
| Spread operator | `[1, ...other, 2]` for arrays, `Point { ...base, x: 10.0 }` for structs | Planned |
| Comptime | `comptime { }` blocks and `comptime fn` executed in VM | Planned |
| Quote | `quote { ... ${interp} ... }` for comptime code generation | Planned |
| Attributes | `#[key = value]` metadata readable at comptime | Planned |
| AST inspection | `ast_of(item)` for read-only AST access at comptime | Planned |
| Module introspection | `module_types()`, `module_functions()` to iterate current module | Planned |
| String interpolation | `` `Hello, {name}!` `` | Planned |
| Modules | File = module, `import` with aliasing and grouping | Planned |
| Dynamic dispatch | `dyn Interface` for runtime polymorphism | Planned |
| Enums as ADTs | Variants with data, struct fields, or unit | Planned |
| Operator overload | Via generic interfaces like `Add<Rhs, Output>` | Planned |
| If let / while let | `if let Some(x) = opt { ... }` for single-pattern convenience | Planned |
| Labeled loops | `'label: for ...` with `break 'label` and `continue 'label` | Planned |
| Type aliases | `type Name = ExistingType;` | Planned |
| Async/await | `async fn`, `.await`, prefix `await`, `Future<T>` | Planned |
| Structured concurrency | `Task.spawn`, `Task.group`, cooperative cancellation | Planned |
| Actors | `actor` type with isolated mutable state, implicit async methods | Planned |
| Pipe operator | `x \|> f(y)` desugars to `f(x, y)` for pipeline-style composition | Planned |
| Package manifest | `nudl.toml` with Go-style source dependencies | Planned |
| Build scripts | `build.nudl` for pre-compilation code generation and configuration | Planned |
| Native codegen | ARM64 backend with Mach-O and ELF output | Planned |
| Comptime VM | SSA bytecode interpreter for compile-time evaluation | Planned |
| LSP | Language Server Protocol for editor integration | Planned |

---

## 3. Language Comparisons

### 3.1 nudl vs Rust

| Aspect | Rust | nudl |
|---|---|---|
| Memory management | Ownership + borrow checker | ARC with compiler-inserted retain/release |
| Lifetimes | Explicit `'a` annotations | None -- ARC handles all lifetimes |
| Shared mutability | Requires `RefCell`, `Mutex`, etc. | Allowed by default (aliased mutation) |
| Polymorphism | Traits (with associated types) | Interfaces (no associated types) |
| Metaprogramming | Proc macros (AST manipulation) | `comptime` blocks (VM-executed) |
| Named arguments | Not supported | First param positional, rest named |
| Trailing closures | Not supported | `f(args) { body }` with implicit `it` |
| Strings | `String` + `&str` (slices) | `string` (ARC'd, no slices) |
| Concurrency | Multi-threaded `async`/`await` with `Send`/`Sync` | Single-threaded cooperative, structured concurrency |
| Pipe operator | Not built-in | `x \|> f` desugars to `f(x)` |
| Package manager | Cargo with crates.io | `nudl.toml` with Go-style source deps |

### 3.2 nudl vs Swift

| Aspect | Swift | nudl |
|---|---|---|
| Expression-based | Partial (ternary, closures) | Fully expression-based (blocks return values) |
| Pattern matching | `switch` with patterns | `match` with exhaustiveness, guards, nested patterns |
| Error handling | `throws`/`try`/`catch` | `Result<T, E>` and `?` operator |
| Generics | Type-erased by default | Monomorphized by default, `dyn Interface` opt-in |
| Metaprogramming | Swift Macros (AST-based) | `comptime` (VM-executed, types as values) |
| String interpolation | `"\(expr)"` | `` `Hello, {expr}!` `` |
| Concurrency | Structured concurrency with actors | Structured concurrency with actors (Swift-inspired) |
| Actors | First-class with `actor` keyword | First-class with `actor` keyword (similar model) |

### 3.3 nudl vs Zig

| Aspect | Zig | nudl |
|---|---|---|
| Memory management | Manual (allocators) | ARC (automatic) |
| Type system | Structural, duck-typed generics | Bounded generics with interfaces |
| Closures | Limited (no heap captures) | Full closures with ARC-captured environment |
| Pattern matching | `switch` on tagged unions | Full `match` with guards, nested patterns |
| Interfaces | Duck typing via `comptime` | Explicit `interface` declarations |
| OOP | None | `impl` blocks with methods, `self`/`mut self` |
| Async | Stackless coroutines, no built-in executor | `async`/`await` with structured concurrency |
| Build system | `build.zig` (Zig code) | `nudl.toml` + `build.nudl` (nudl code) |

### 3.4 Operator Precedence

| Prec | Category       | Operators                              | Assoc  |
|------|----------------|----------------------------------------|--------|
| 15   | Primary        | `.` `[]` `()`                          | Left   |
| 14   | Postfix        | `?` `.await`                           | Postfix|
| 13   | Prefix         | `-` (unary) `!`                        | Prefix |
| 12   | Cast           | `as`                                   | Left   |
| 11   | Multiplicative | `*` `/` `%`                            | Left   |
| 10   | Additive       | `+` `-`                                | Left   |
| 9    | Shift          | `<<` `>>`                              | Left   |
| 8    | Bitwise AND    | `&`                                    | Left   |
| 7    | Bitwise XOR    | `^`                                    | Left   |
| 6    | Bitwise OR     | `\|`                                   | Left   |
| 5    | Comparison     | `==` `!=` `<` `>` `<=` `>=`           | None   |
| 4    | Logical AND    | `&&`                                   | Left   |
| 3    | Logical OR     | `\|\|`                                 | Left   |
| 2    | Range          | `..` `..=`                             | None   |
| 1    | Pipe           | `\|>`                                  | Left   |
| 0    | Assignment     | `=` `+=` `-=` `*=` `/=` `%=`          | Right  |

Comparison operators are non-chaining. Prefix `await` is a keyword expression, not an operator.

---
