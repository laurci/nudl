# nudl Compiler Internals

Compiler architecture, SSA bytecode design, virtual machine, ARC implementation, and native code generation.

## Contents

| File | Sections | Description |
|------|----------|-------------|
| [architecture-and-pipeline.md](architecture-and-pipeline.md) | 1–2 | Crate map, responsibilities, data flow, full compilation pipeline |
| [ssa-bytecode.md](ssa-bytecode.md) | 3 | SSA IR principles, structure, instruction set, examples |
| [vm-and-comptime.md](vm-and-comptime.md) | 4–5 | VM architecture, execution loop, comptime model, quote blocks, async lowering |
| [arc-and-codegen.md](arc-and-codegen.md) | 6–7 | ARC retain/release, object layout, ARM64 backend, register allocation |
| [tools-and-reference.md](tools-and-reference.md) | 8–13 | Binary packers, nudl-core, LSP, CLI, end-to-end example, design tradeoffs |

## See Also

- [Feature Overview](../features/README.md) — High-level feature tour and comparisons
- [Language Specification](../spec/README.md) — Normative language spec
