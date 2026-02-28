# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

nudl is a compiled programming language: "The power and syntax of Rust, with the memory management of Swift and the metaprogramming of Zig." Statically-typed, ARC-managed (no borrow checker, no GC), single-threaded in v1, with Zig-style comptime metaprogramming.

**Status:** Core pipeline works end-to-end (lexer → parser → type checker → SSA IR → LLVM backend → native binary). See `STATUS.md` for detailed feature-level tracking.

## Build Commands

```bash
cargo build                     # Build default members (nudl-cli, nudl-lsp)
cargo build --workspace         # Build all 7 crates
cargo test --workspace          # Run all tests
cargo test -p nudl-ast          # Run tests for a single crate
cargo check --workspace         # Type-check without building
cargo run --bin nudl-cli        # Run the CLI binary
cargo run --bin nudl-lsp        # Run the LSP binary
```

All crates use Rust edition 2024, resolver 3. Requires LLVM 18 installed (`brew install llvm@18` on macOS). Set `LLVM_SYS_181_PREFIX=/opt/homebrew/opt/llvm@18` when building.

## Architecture

Compilation pipeline flows through crates in this order:

```
Source → nudl-ast (lex/parse) → nudl-bc (type-check + SSA bytecode)
    → nudl-vm (comptime eval, feeds generated code back to nudl-bc)
    → nudl-backend-llvm (SSA → LLVM IR → object file → executable)
```

### Crate Responsibilities

| Crate | Role |
|---|---|
| **nudl-core** | Foundation: spans, diagnostics, type representations, interning. No dependencies. |
| **nudl-ast** | Lexer + recursive-descent/Pratt parser → untyped AST |
| **nudl-bc** | Hindley-Milner type inference, interface resolution, monomorphization, AST → SSA bytecode, ARC retain/release insertion |
| **nudl-vm** | Register-based VM executing SSA bytecode for comptime evaluation. Step-limited, no I/O. |
| **nudl-backend-llvm** | SSA bytecode → LLVM IR via Inkwell → native object file + system linker. Multi-arch, multi-platform. |
| **nudl-cli** | CLI frontend: `build`, `run`, `check`, `fmt` subcommands |
| **nudl-lsp** | Language Server Protocol server for editor integration |

### Key Design Decisions

- **SSA bytecode** is the shared IR — same format consumed by both the VM (comptime) and the native backend
- **ARC is non-atomic** in v1 (single-threaded). Compiler inserts retain/release in the bytecode layer.
- **Comptime** code runs in the VM, can produce values (serialized to constants) or new code via `quote { ... ${} ... }` blocks that get re-injected into the pipeline for type-checking
- **Interfaces** (not traits) — no associated types; generics fill that role (e.g., `Iterator<T>`, `Index<Idx, Output>`)
- **Reference types** (structs, enums, string, `T[]`, `Map<K,V>`, closures, `dyn Interface`) are heap-allocated and ARC'd. **Value types** (primitives, tuples, `[T; N]`) are stack-allocated and copied.

## Specification Documents

Documentation is organized into three directories under `docs/` (see `docs/README.md` for the full index):

- `docs/features/` — Feature overview, comparisons, example programs (4 files)
- `docs/spec/` — Normative language specification: types, expressions, control flow, comptime, etc. (7 files)
- `docs/internals/` — Compiler architecture, SSA bytecode, VM, ARC, native codegen (5 files)

Each directory has a `README.md` index linking to all files with descriptions. These are the source of truth for language semantics. All implementation should conform to these specs.

## Testing the Compiler

When testing `.nudl` files with `cargo run --bin nudl -- build/run`, place any compiled output binaries in `./tmp/` or `/tmp/`, never in the project root.

## Implementation Tracking

**Always update `STATUS.md` after implementing a feature or making significant progress.** Mark items as `[x]` (complete), `[~]` (partial), or add new entries as needed. Include a brief note describing what was done.
