# nudl -- Compiler Architecture and Execution Model

> The power and syntax of Rust, with the memory management of Swift and the
> metaprogramming of Zig.

This document describes the internal architecture of the nudl compiler: how
source code flows through the pipeline, the design of the intermediate
representations, how compile-time evaluation works, how native code is
generated, and how each crate in the workspace contributes to the whole.

---

## 1. Compiler Architecture Overview

### 1.1 Crate Map

The nudl compiler is a Rust workspace composed of nine crates. The dependency
graph flows top-down:

```
                          nudl-cli          nudl-lsp
                            |                  |
                            +--------+---------+
                                     |
                              nudl-backend-arm64
                               /           \
                       nudl-packer-macho   nudl-packer-elf
                               \           /
                                nudl-vm
                                  |
                                nudl-bc
                                  |
                                nudl-ast
                                  |
                                nudl-core
```

### 1.2 Crate Responsibilities

| Crate | Role |
|---|---|
| `nudl-core` | Shared foundation: source locations, spans, diagnostics, error types, type representations, string/type interning |
| `nudl-ast` | Lexer and parser. Transforms source text into an untyped AST |
| `nudl-bc` | Type checker and SSA bytecode generator. Transforms the AST into type-checked SSA bytecode IR. Inserts ARC retain/release operations |
| `nudl-vm` | Register-based virtual machine. Interprets SSA bytecode for comptime evaluation |
| `nudl-backend-arm64` | Native code generator. Lowers SSA bytecode to ARM64 machine code |
| `nudl-packer-macho` | Produces Mach-O executables for macOS/Darwin targets |
| `nudl-packer-elf` | Produces ELF executables for Linux targets |
| `nudl-cli` | Command-line frontend (`nudl build`, `nudl run`, `nudl check`, `nudl fmt`) |
| `nudl-lsp` | Language Server Protocol implementation for editor integration |

### 1.3 Data Flow

The full compilation pipeline, including dependency resolution, build scripts,
and comptime feedback loops:

```
  nudl.toml + Source (.nudl files)
       |
       v
  +--------------------+
  | Dependency         |    fetch deps to .nudl/deps/
  | Resolution         |    read .nudl/deps.lock
  +--------------------+
       |
       v
  +--------------------+
  | Build Script       |    compile + execute build.nudl
  | (nudl-vm, extended)|    produces .nudl/generated/ files + defines
  +--------------------+
       |
       v
  +----------+    tokens    +----------+    untyped AST
  |  Lexer   | ----------> |  Parser  | ------------+
  +----------+  (nudl-ast) +----------+  (nudl-ast) |
                                                     v
                                              +--------------+
                        typed SSA bytecode    | Type Checker  |
                   +------------------------  | + BC Lowering |
                   |                          +--------------+
                   |                            (nudl-bc)
                   v                                ^
            +------------+                          |
            |  Comptime  |    new AST fragments     |
            |  VM        | -------------------------+
            +------------+    (re-injected into type checking)
            (nudl-vm)
                   |
                   | (non-comptime BC)
                   v
          +----------------+
          | ARM64 Backend  |
          +----------------+
          (nudl-backend-arm64)
                   |
                   v
       +-----------+-----------+
       |                       |
  +-----------+          +-----------+
  | Mach-O    |          | ELF       |
  | Packer    |          | Packer    |
  +-----------+          +-----------+
       |                       |
       v                       v
   a.out (macOS)         a.out (Linux)
```

---

## 2. Compilation Pipeline

### 2.1 Source to Tokens (Lexer)

The lexer in `nudl-ast` performs a single linear scan over UTF-8 source text,
producing a flat stream of tokens. Each token carries its `Span` (byte offset
range into the source) for diagnostics and LSP integration.

**Token categories:**

| Category | Examples |
|---|---|
| Keywords | `fn`, `let`, `mut`, `struct`, `enum`, `interface`, `impl`, `match`, `if`, `else`, `for`, `while`, `loop`, `return`, `break`, `continue`, `comptime`, `pub`, `import`, `defer`, `weak`, `dyn`, `as` |
| Literals | Integer (`42`, `0xFF`), float (`3.14`), string (`"hello"`), template string (`` `x={x}` ``), char (`'a'`), bool (`true`/`false`) |
| Identifiers | `foo`, `Point`, `T` |
| Operators | `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `\|\|`, `!`, `&`, `\|`, `^`, `<<`, `>>`, `=`, `+=`, `-=`, `..`, `..=`, `...`, `->`, `=>`, `?`, `\|>` |
| Delimiters | `(`, `)`, `{`, `}`, `[`, `]`, `,`, `:`, `;`, `::`, `.` |
| Special | `EOF`, `Newline` (significant in some contexts), `Comment` |

**Template string handling:** The lexer recognizes backtick-delimited template strings and emits
them as a sequence of `TemplateStringStart`, literal segments, `TemplateStringExpr` (the
`{expr}` parts, which are sub-lexed), and `TemplateStringEnd`. This keeps the parser
free of string-interpolation complexity.

### 2.2 Tokens to AST (Parser)

The parser consumes the token stream and builds an untyped AST using recursive
descent with Pratt parsing for expressions.

**Parser design:**

- **Top-level items** (functions, structs, enums, interfaces, impl blocks,
  imports, comptime blocks) are parsed by dedicated functions dispatched on the
  leading keyword.
- **Expressions** use a Pratt parser (precedence climbing). Each operator has a
  binding power that determines grouping without explicit precedence tables in
  the grammar. This naturally handles prefix, infix, and postfix operators,
  including method calls (`.`), field access, indexing (`[]`), and the `?`
  operator.
- **Statements** are expressions optionally followed by `;`. A trailing
  expression without `;` is the block's return value.

**AST node taxonomy:**

```
Item
  |- FnDef          { name, generics, params, ret_type, body, is_comptime, is_async }
  |- StructDef      { name, generics, fields }
  |- EnumDef        { name, generics, variants }
  |- InterfaceDef   { name, generics, methods }
  |- ImplBlock      { target_type, interface, generics, methods }
  |- ImportDecl     { path, alias, group }
  |- ComptimeBlock  { body }
  |- ActorDef       { name, generics, fields, methods }

Expr
  |- Literal        { kind: Int|Float|String|TemplateString|Char|Bool }
  |- Ident          { name }
  |- Binary         { op, lhs, rhs }
  |- Unary          { op, operand }
  |- Call           { callee, args (positional + named), trailing_lambda }
  |- MethodCall     { receiver, method, args }
  |- FieldAccess    { object, field }
  |- Index          { object, index }
  |- If             { condition, then_branch, else_branch }
  |- Match          { scrutinee, arms: [(Pattern, guard?, body)] }
  |- Block          { stmts, tail_expr }
  |- Closure        { params, body }
  |- For            { binding, iterator, body }
  |- While          { condition, body }
  |- Loop           { body }
  |- Return         { value }
  |- Break          { value }
  |- Continue
  |- Let            { pattern, type_ann, initializer, is_mut }
  |- Assign         { target, value }
  |- Defer          { body }
  |- Spread         { expr }
  |- Try            { expr }  (the ? operator)
  |- Cast           { expr, target_type }
  |- Await          { expr, is_postfix }
  |- AsyncBlock     { body }
  |- StructLiteral  { name, fields, spread }
  |- ArrayLiteral   { elements }
  |- TupleLiteral   { elements }
  |- Path           { segments }  (e.g., std::collections::Map)

Pattern
  |- Literal        { value }
  |- Binding        { name, is_mut }
  |- Wildcard
  |- Tuple          { patterns }
  |- Struct         { name, field_patterns, has_rest }
  |- Enum           { path, inner_pattern }
  |- Or             { patterns }
  |- Range          { start, end, inclusive }
```

Every AST node is tagged with its `Span` so that later phases can produce
accurate diagnostics pointing back to source locations.

**Pipe operator desugaring:** The `|>` operator is desugared during parsing
into a `Call` node. `x |> f(y)` becomes `Call(f, [x, y])`. No `Pipe` AST node
survives to the type-checking phase — this is purely syntactic sugar.

### 2.3 AST to SSA Bytecode (Type Checker + BC Lowering)

The `nudl-bc` crate performs two tightly coupled passes:

1. **Type checking** -- annotates every expression with a concrete type.
2. **SSA bytecode generation** -- lowers the typed AST into a flat, typed SSA
   intermediate representation.

#### 2.3.1 Type Checking

nudl uses **Hindley-Milner bidirectional type inference**:

- **Inference (bottom-up):** Literal types are known. Variable types are
  inferred from their initializers. Operator result types are determined by the
  operand types and the operator interface (`Add<Rhs, Output>`, etc.).
- **Checking (top-down):** Function return types, parameter types, and explicit
  annotations push expected types downward. This resolves ambiguities like
  integer literal widths (`42` could be `i32` or `i64` depending on context).
- **Unification:** When two types must agree, the checker unifies them. If a
  type variable meets a concrete type, it is bound. If two concrete types
  conflict, a type error is reported.

**Generics and monomorphization:** Generic functions and types are instantiated
for each distinct set of type arguments used at call sites. The checker
maintains a monomorphization cache -- `(generic_def_id, [concrete_type_args])`
maps to the specialized version. Each monomorphized instance goes through full
type checking as if the generic parameters were replaced by their concrete
types.

**Interface resolution:** When a function has a bound like `T: Ord`, the checker
looks up the `Ord` implementation for the concrete type substituted for `T`.
This lookup happens at monomorphization time. If no implementation exists, a
compile error is emitted with the instantiation site in the diagnostic chain.

#### 2.3.2 SSA Bytecode Generation

After type checking, each function body is lowered into SSA bytecode. SSA
(Static Single Assignment) means every virtual register is assigned exactly
once. This form simplifies analysis and optimization.

The lowering process:

1. Create an entry basic block for the function.
2. Walk the typed AST, emitting instructions into the current basic block.
3. At control flow splits (`if`, `match`, `loop`), create new basic blocks
   and emit branch/jump instructions.
4. At control flow joins, insert phi nodes to merge values from different
   predecessors.
5. Insert ARC retain/release instructions (see Section 6).

Details of the SSA bytecode format are in Section 3.

### 2.4 SSA BC to VM Execution (Comptime)

`comptime` blocks and `comptime fn` definitions are compiled to SSA bytecode
just like runtime code, then executed immediately in the VM (`nudl-vm`). The
VM can produce:

- **Constant values** that are embedded into the calling function's bytecode.
- **New AST fragments** (structs, functions, impl blocks) that are re-injected
  into the compilation pipeline for type checking and further lowering.

See Section 5 for the full comptime execution model.

### 2.5 SSA BC to ARM64 Machine Code

The `nudl-backend-arm64` crate translates SSA bytecode into ARM64 instructions.
This involves instruction selection, register allocation, and ABI conformance.
See Section 7 for details.

### 2.6 Machine Code to Binary

The packer crates (`nudl-packer-macho`, `nudl-packer-elf`) take the emitted
machine code and data sections and wrap them in the appropriate executable
format. See Section 8 for details.

---
