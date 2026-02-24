## 8. Binary Packers

### 8.1 Mach-O Packer (`nudl-packer-macho`)

The Mach-O packer produces executables for macOS (Darwin) on ARM64. It writes
the binary format directly, without depending on a system linker.

**Section layout:**

```
Mach-O Header (magic, cputype, filetype)
Load Commands
  LC_SEGMENT_64 "__TEXT"
    __text          (executable code)
    __stubs         (lazy symbol stubs)
    __stub_helper   (stub helper routines)
    __cstring       (C string constants)
    __const         (read-only constants)
  LC_SEGMENT_64 "__DATA"
    __data          (initialized mutable data)
    __bss           (zero-initialized data)
    __la_symbol_ptr (lazy symbol pointers)
    __got           (global offset table)
  LC_SEGMENT_64 "__LINKEDIT"
    symbol table
    string table
    relocation entries
  LC_SYMTAB
  LC_DYSYMTAB
  LC_LOAD_DYLINKER
  LC_MAIN (entry point offset)
  LC_LOAD_DYLIB (libSystem.B.dylib -- for syscalls)
```

The packer resolves internal relocations (function-to-function calls within the
nudl program) at pack time. External relocations (calls to the ARC runtime or
system libraries) go through the GOT and lazy binding stubs.

### 8.2 ELF Packer (`nudl-packer-elf`)

The ELF packer produces executables for Linux on ARM64 (aarch64).

**Section layout:**

```
ELF Header (e_ident, e_type = ET_EXEC, e_machine = EM_AARCH64)
Program Headers
  PT_LOAD (R+X): .text, .rodata
  PT_LOAD (R+W): .data, .bss, .got, .got.plt
  PT_DYNAMIC: dynamic linking information
  PT_INTERP: /lib/ld-linux-aarch64.so.1
Section Headers
  .text         (executable code)
  .rodata       (read-only data, string constants)
  .data         (initialized mutable data)
  .bss          (zero-initialized data)
  .got          (global offset table)
  .got.plt      (PLT GOT entries)
  .plt          (procedure linkage table)
  .rela.dyn     (dynamic relocations)
  .rela.plt     (PLT relocations)
  .dynsym       (dynamic symbol table)
  .dynstr       (dynamic string table)
  .symtab       (full symbol table)
  .strtab       (string table)
  .shstrtab     (section name strings)
```

Both packers share the same interface: they accept a list of code sections,
data sections, and symbol/relocation records from the backend, and produce a
self-contained executable binary.

> **v2 Note:** Debug symbol emission (DWARF) is deferred to v2. In v1, stack
> traces display instruction addresses only. The `--debug` flag is reserved for
> future use.

---

## 9. Core Library (`nudl-core`)

### 9.1 Source Locations and Spans

All diagnostic information traces back to source code through the `Span` type:

```
Span
  file_id: FileId       // index into the source file table
  start:   u32          // byte offset of the start
  end:     u32          // byte offset of the end (exclusive)

SourceFile
  id:      FileId
  path:    PathBuf
  content: String       // full source text (owned)
  lines:   Vec<u32>     // byte offsets of line starts (for line:column lookup)
```

The `SourceMap` structure manages loaded source files and provides
`span_to_location(Span) -> (file, line, column)` lookups for diagnostic
rendering.

### 9.2 Diagnostic System

Diagnostics are structured reports with severity, message, source location,
labels, and notes:

```
Diagnostic
  severity:  Error | Warning | Note | Help
  code:      String            // e.g., "E0301"
  message:   String            // primary message
  labels:    Vec<Label>        // source spans with annotations
  notes:     Vec<String>       // additional context

Label
  span:     Span
  message:  String
  style:    Primary | Secondary
```

Diagnostics are collected during compilation and rendered to stderr in a
format inspired by rustc:

```
error[E0301]: type mismatch
  --> src/main.nudl:10:20
   |
10 |     let x: i32 = "hello";
   |            ---   ^^^^^^^ expected `i32`, found `string`
   |            |
   |            expected due to this annotation
```

### 9.3 Type Representations

Types are represented as interned handles into a global type table. This allows
type comparison by integer equality rather than structural recursion:

```
TypeId  = u32  (index into the type table)

TypeKind
  |- Primitive(PrimKind)          // i32, f64, bool, char, ()
  |- String
  |- Tuple(Vec<TypeId>)
  |- Array(TypeId)                // dynamic array T[]
  |- FixedArray(TypeId, u64)      // [T; N]
  |- Map(TypeId, TypeId)          // Map<K, V>
  |- Struct(StructDefId)
  |- Enum(EnumDefId)
  |- Function(Vec<TypeId>, TypeId)  // (params) -> return
  |- Generic(GenericParamId)      // unresolved generic parameter
  |- Dyn(InterfaceId)             // dyn Interface
  |- WeakRef(TypeId)              // weak T
  |- Future(TypeId)               // Future<T>
  |- Actor(ActorDefId)            // actor type
  |- Error                        // poison type (suppresses cascading errors)
```

### 9.4 Interning

Both strings and types are interned to avoid redundant allocations and enable
cheap comparisons:

- **String interning.** Identifiers, field names, and module paths are stored
  once in a global `StringInterner` and referenced by `Symbol` (a `u32`
  handle).
- **Type interning.** Constructed types (`Tuple([i32, f64])`) are hashed and
  deduplicated. The `TypeInterner` returns a `TypeId` that uniquely identifies
  the structural type.

Interning is essential for performance during type checking, where the same
types are constructed and compared millions of times in large programs.

---

## 10. LSP Integration (`nudl-lsp`)

### 10.1 Architecture

The LSP server reuses the compilation pipeline up through type checking. It
does not run the native backend or packers. The LSP crate owns a persistent
`CompilationState` that caches:

- Parsed ASTs for all open files.
- Type-checking results (typed ASTs, symbol tables, interface resolution
  results).
- Diagnostic sets per file.

### 10.2 Incremental Compilation

When a file changes, the LSP server:

1. Re-lexes and re-parses only the changed file.
2. Invalidates type-checking results for the changed file and its dependents
   (computed from the import graph).
3. Re-runs type checking on invalidated files.
4. Publishes updated diagnostics to the editor.

The import graph is maintained as a DAG. A change to `math/vector.nudl` only
invalidates files that import `math::vector`, not the entire project.

### 10.3 Supported Features

| LSP Feature | Implementation |
|---|---|
| **Diagnostics** | Published on save/change from the type checker's diagnostic output |
| **Go to Definition** | Symbol table lookup: find the `Span` of the definition site for the symbol under the cursor |
| **Find References** | Reverse symbol table lookup: find all `Span`s where a given definition is referenced |
| **Hover** | Type information and documentation: display the inferred type and doc comments for the symbol under the cursor |
| **Completion** | Context-aware: after `.`, suggest methods and fields of the receiver type; at top level, suggest keywords and visible symbols |
| **Signature Help** | When inside a function call's argument list, display the function's parameter names and types, highlighting the current parameter |

### 10.4 Comptime and the LSP

Comptime blocks pose a challenge for incremental compilation: changing a
comptime block may generate different code, invalidating types across the
project. The LSP handles this conservatively:

- On change to a comptime block, the LSP re-executes it and compares the
  generated AST fragments to the previously generated ones.
- If the output changed, dependents are invalidated. If not, the cache holds.

---

## 11. CLI Frontend (`nudl-cli`)

### 11.1 Command Structure

```
nudl build [--target <target>] [--output <path>] [<source>]
    Compile a nudl program to a native executable.
    Targets: aarch64-apple-darwin (default on macOS), aarch64-linux-gnu.
    If no <source> is given, looks for nudl.toml in the current directory
    and uses the configured entry point (default: main.nudl).

nudl run [<source>] [-- <args>...]
    Compile and immediately execute. Equivalent to `nudl build` + run.

nudl check [<source>]
    Parse and type-check without generating machine code. Useful for CI
    and editor integration outside the LSP.

nudl fmt [<source>]
    Format nudl source code according to the canonical style.
```

### 11.5 Target Platform Support

| Version | Target | Status |
|---------|--------|--------|
| v1 | `aarch64-apple-darwin` | Planned |
| v1 | `aarch64-linux-gnu` | Planned |
| v2 | `x86_64-apple-darwin` | Future |
| v2 | `x86_64-linux-gnu` | Future |

The x86-64 backend (`nudl-backend-x86_64`) is planned for v2. It will consume
the same SSA bytecode IR as the ARM64 backend, requiring only a new instruction
selector and register allocator targeting the x86-64 ISA and System V / macOS
x86-64 calling conventions.

### 11.2 Pipeline Orchestration

The CLI drives the pipeline end-to-end:

```
fn build(source_path, target, output_path):
    // Phase 0: Manifest and dependency resolution
    manifest = load_manifest(source_path)  // nudl.toml (optional)
    if manifest:
        resolve_dependencies(manifest)     // fetch to .nudl/deps/
        source_path = manifest.entry       // default: main.nudl

    // Phase 0.5: Build script (if build.nudl exists)
    if exists("build.nudl"):
        build_bc = parse_and_lower("build.nudl")
        build_result = vm_execute(build_bc, mode: "build_script")
        // build_result contains: generated files, defines, flags
        apply_build_flags(build_result.flags)

    // Note: if build.nudl execution fails (panic or main() returns Err),
    // the build is aborted here — no further phases run.

    // Phase 1: Parse (including generated files and dependencies)
    source_map = load_sources(source_path, deps: ".nudl/deps/",
                              generated: ".nudl/generated/")
    ast = parse_all(source_map)
    report_diagnostics(ast.diagnostics)
    if has_errors: exit(1)

    // Phase 2: Type check + BC generation (with comptime loop)
    bc_program = type_check_and_lower(ast, source_map,
                                       defines: build_result.defines)
    report_diagnostics(bc_program.diagnostics)
    if has_errors: exit(1)

    // Phase 3: Native backend
    machine_code = match target:
        AArch64 => arm64_codegen(bc_program)
    report_diagnostics(machine_code.diagnostics)
    if has_errors: exit(1)

    // Phase 4: Pack
    match target:
        AArch64_Apple_Darwin => pack_macho(machine_code, output_path)
        AArch64_Linux_Gnu    => pack_elf(machine_code, output_path)
```

If `build.nudl` execution fails — either by panic or by `main()` returning an
`Err` value — the build is aborted with a diagnostic pointing to the build
script failure. The compiler does not proceed to parse or compile the main
project sources.

### 11.3 Dependency Resolution

When a `nudl.toml` manifest is present, the compiler resolves dependencies
before parsing:

1. **Read manifest.** Parse `nudl.toml` and extract the `[dependencies]`
   section.
2. **Check lock file.** If `.nudl/deps.lock` exists, verify that all
   dependencies match the locked versions.
3. **Fetch missing dependencies.** For each dependency not present in
   `.nudl/deps/`:
   - Git-clone the repository at the specified version (tag, commit, or
     default branch).
   - Verify the fetched content hash matches the lock file (if locked).
   - Store the source in `.nudl/deps/<package-name>/`.
4. **Resolve transitive dependencies.** Each dependency may itself have a
   `nudl.toml` with dependencies. Resolve recursively.
5. **Single-version check.** Verify that no package appears at multiple
   versions in the resolved graph. Diamond conflicts are reported as compile
   errors.
6. **Update lock file.** Write `.nudl/deps.lock` with the resolved commit
   hashes and content hashes.

**Lock file format** (`.nudl/deps.lock`):

```
[[package]]
name = "nudl-http"
source = "github.com/user/nudl-http"
commit = "abc1234def5678"
content_hash = "sha256:..."

[[package]]
name = "nudl-json"
source = "github.com/user/nudl-json"
tag = "v1.2.0"
commit = "789abc0def1234"
content_hash = "sha256:..."
```

### 11.4 Diagnostic Rendering

The CLI renders diagnostics to stderr using ANSI colors when the terminal
supports them. Diagnostics are sorted by severity (errors first, then warnings)
and by source location. The exit code is 0 if no errors occurred, 1 otherwise.
Warnings do not cause a non-zero exit code unless `--deny-warnings` is passed.

---

## 12. End-to-End Example

To illustrate the complete pipeline, here is how a simple program flows through
every stage.

### 12.1 Source

```nudl
fn factorial(n: u64) -> u64 {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

fn main() {
    let result = factorial(10);
    println(`10! = {result}`);
}
```

### 12.2 Tokens (abbreviated)

```
Fn, Ident("factorial"), LParen, Ident("n"), Colon, Ident("u64"), RParen,
Arrow, Ident("u64"), LBrace, If, Ident("n"), Le, Int(1), LBrace, Int(1),
RBrace, Else, LBrace, Ident("n"), Star, Ident("factorial"), LParen,
Ident("n"), Minus, Int(1), RParen, RBrace, RBrace, ...
```

### 12.3 AST (abbreviated)

```
Module
  FnDef "factorial"
    params: [(n, u64)]
    return: u64
    body: If
      cond: Binary(Le, Ident(n), Literal(1))
      then: Block [Literal(1)]
      else: Block [Binary(Mul, Ident(n),
                     Call(Ident(factorial),
                       [Binary(Sub, Ident(n), Literal(1))]))]
  FnDef "main"
    body: Block
      Let(result, Call(Ident(factorial), [Literal(10)]))
      Call(Ident(println), [TemplateString("10! = ", Ident(result))])
```

### 12.4 SSA Bytecode

```
function factorial(r0: u64) -> u64:

  block entry(r0: u64):
    r1 = Const(1: u64)
    r2 = Le(r0, r1)
    Branch(r2, base[], recurse[])

  block base():
    r3 = Const(1: u64)
    Jump(exit[r3])

  block recurse():
    r4 = Const(1: u64)
    r5 = Sub(r0, r4)
    r6 = Call(factorial, [r5])
    r7 = Mul(r0, r6)
    Jump(exit[r7])

  block exit(r8: u64):
    Return(r8)


function main() -> ():

  block entry():
    r0 = Const(10: u64)
    r1 = Call(factorial, [r0])
    r2 = Const("10! = ": string)
    Retain(r2)
    r3 = Call(u64_to_string, [r1])
    r4 = Call(string_concat, [r2, r3])
    Release(r3)
    Release(r2)
    r5 = Call(println, [r4])
    Release(r4)
    r6 = ConstUnit()
    Return(r6)
```

### 12.5 ARM64 Assembly (simplified)

```asm
_factorial:
    STP  X29, X30, [SP, #-16]!
    MOV  X29, SP
    CMP  X0, #1                 ; n <= 1?
    B.HI .recurse
    MOV  X0, #1                 ; return 1
    LDP  X29, X30, [SP], #16
    RET

.recurse:
    STR  X0, [SP, #-16]!       ; save n
    SUB  X0, X0, #1            ; n - 1
    BL   _factorial             ; factorial(n - 1)
    LDR  X1, [SP], #16         ; restore n
    MUL  X0, X1, X0            ; n * factorial(n - 1)
    LDP  X29, X30, [SP], #16
    RET
```

### 12.6 Binary Output

The packer wraps the machine code into a Mach-O (or ELF) executable with:

- `__TEXT/__text` containing `_factorial` and `_main`.
- `__DATA/__const` containing the string literal `"10! = "`.
- Symbol table entries for `_factorial`, `_main`, `_println`, etc.
- An `LC_MAIN` load command pointing to `_main` as the entry point.
- An `LC_LOAD_DYLIB` for `libSystem.B.dylib` to resolve `_println` (which
  ultimately calls `write(2)` via the system library).

The result is a standalone executable:

```
$ nudl build factorial.nudl -o factorial
$ ./factorial
10! = 3628800
```

---

## 13. Design Decisions and Trade-offs

### 13.1 One IR to Rule Them All

Using the same SSA bytecode for both the comptime VM and the native backend
simplifies the architecture: there is one lowering pass, one set of
optimizations, and one instruction format to maintain. The trade-off is that
the IR must be general enough to support efficient interpretation (favoring
simplicity) and efficient native codegen (favoring low-level control). The
typed, register-based SSA design achieves a reasonable middle ground.

### 13.2 Block Parameters vs Phi Nodes

Traditional SSA uses phi nodes at the top of basic blocks. nudl's IR uses
block parameters instead -- a design borrowed from MLIR and Cranelift. Block
parameters make the data flow at control-flow edges explicit: a jump to a block
lists the values it passes, and the block declares the parameters it receives.
This avoids the ambiguity of phi nodes (which implicitly reference predecessor
blocks) and simplifies both the VM interpreter and the register allocator.

### 13.3 Register-Based VM

A register-based VM was chosen over a stack-based VM because SSA bytecode
already names its values with virtual registers. A stack-based VM would require
an additional lowering step to convert register references into stack
operations. The register-based design also produces fewer instructions per
operation (no dup/swap) and maps more directly to the native backend's register
allocation.

### 13.4 Direct Binary Generation

nudl generates Mach-O and ELF binaries directly rather than emitting assembly
or object files for an external assembler/linker. This eliminates external
toolchain dependencies and gives the compiler full control over the binary
layout. The trade-off is the complexity of implementing the binary format
writers, but both Mach-O and ELF are well-documented and the required subset
(static executables with minimal dynamic linking) is manageable.

### 13.5 ARC vs Tracing GC vs Borrow Checker

ARC was chosen as the memory management strategy because:

- **Deterministic.** Objects are freed immediately when the last reference
  drops, enabling predictable performance (no GC pauses).
- **Simple mental model.** Programmers reason about ownership without lifetime
  annotations.
- **Efficient for most workloads.** The retain/release overhead is constant
  per operation; the optimization passes eliminate most redundant operations.

The trade-off compared to a borrow checker is the possibility of reference
cycles (mitigated by `weak` and compile-time cycle detection) and the runtime
overhead of reference counting (mitigated by retain/release elision). The
trade-off compared to tracing GC is the need for `weak` annotations and the
inability to automatically collect cycles.
