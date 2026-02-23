# nudl Language Specification

Normative specification for the nudl programming language (Version 0.1.0 — Draft).

## Contents

| File | Sections | Description |
|------|----------|-------------|
| [lexical-structure.md](lexical-structure.md) | 1–2 | Introduction, notation, tokens, keywords, literals, operators |
| [type-system-and-memory.md](type-system-and-memory.md) | 3–4 | Full type system and ARC memory model |
| [declarations-and-expressions.md](declarations-and-expressions.md) | 5–6 | Items, declarations, all expression forms |
| [control-flow-and-patterns.md](control-flow-and-patterns.md) | 7–10 | Statements, pattern matching, error handling, control flow |
| [functions-and-modules.md](functions-and-modules.md) | 11–12 | Functions, closures, modules, packages, visibility |
| [comptime.md](comptime.md) | 13 | Compile-time evaluation model |
| [reference.md](reference.md) | 14–18 | Attributes, built-in interfaces, grammar summary, concurrency, appendices |
| [ffi.md](ffi.md) | 19 | Foreign function interface: extern blocks, FFI types, linking, callbacks |
| [stdlib.md](stdlib.md) | 20 | Standard library: Error/From interfaces, Option/Result methods, Set, math, strings, file I/O |

## See Also

- [Feature Overview](../features/README.md) — High-level feature tour and comparisons
- [Compiler Internals](../internals/README.md) — Compiler architecture and execution model
