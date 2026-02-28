## 6. ARC Implementation Details

### 6.1 Value Types vs Reference Types

The type system classifies every type into one of two categories:

**Value types** (stack-allocated, copied on assignment):
- Primitives: `i8`..`i64`, `u8`..`u64`, `f32`, `f64`, `bool`, `char`, `()`
- Tuples of value types: `(i32, f64)`
- Fixed-size arrays of value types: `[i32; 4]`

**Reference types** (heap-allocated, reference-counted):
- Structs, enums, `string`
- Dynamic arrays: `T[]`
- Maps: `Map<K, V>`
- Closures (capture environment on the heap)
- Dynamic dispatch objects: `dyn Interface`

### 6.2 Retain/Release Insertion

The BC generator inserts ARC operations according to these rules:

1. **Allocation.** `Alloc` creates an object with refcount = 1. No explicit
   `Retain` needed.
2. **Assignment/binding.** When a reference-type value is assigned to a new
   binding or stored into a field, `Retain` the new value, then `Release` the
   old value (if overwriting).
3. **Function arguments — Caller-Retain / Callee-Release.**

   For reference-type function arguments, the calling convention is:
   1. **Caller** emits `Retain` for each reference-type argument before the `Call` instruction.
   2. **Callee** takes ownership of the retain and emits `Release` for each reference-type parameter at function exit (or when the parameter binding goes out of scope).

   This convention minimizes retain/release pairs when the callee uses the argument multiple times, as no intermediate retain is needed within the function body.
4. **Return values.** The callee returns with refcount already accounting for
   the caller's ownership. No extra retain needed.
5. **Scope exit.** When a binding goes out of scope, `Release` is emitted.
   `defer` blocks execute before scope-exit releases.

### 6.3 Retain/Release Optimization

Naive insertion produces redundant operations. The BC optimizer performs these
passes:

- **Balanced pair elimination.** A `Retain` immediately followed by a `Release`
  of the same register (with no intervening use) is eliminated.
- **Move semantics (planned, v2).** When the source of an assignment is not
  used after the assignment, the `Retain`/`Release` pair could be elided --
  the reference would be moved rather than copied. This optimization is not
  implemented in v1; all assignments generate retain/release pairs.
- **Last-use analysis (planned, v2).** If a binding's last use is as an
  argument to a function call, the `Release` at scope exit could be eliminated
  and the function would take ownership. This optimization is deferred to v2.

#### v1 Optimization Passes

The SSA bytecode undergoes the following optimization passes before native code
generation:

1. **Retain/release elision** — removes balanced, consecutive retain/release
   pairs on the same register.
2. **Constant folding** — evaluates arithmetic, comparison, and logical
   operations on constant operands at compile time, replacing them with `Const`
   instructions.
3. **Dead code elimination (DCE)** — removes basic blocks that are unreachable
   from the entry block, and instructions whose results are never used.
4. **Basic inlining** — inlines small, non-recursive functions (heuristic:
   fewer than ~16 SSA instructions) at their call sites, then re-runs
   retain/release elision on the inlined code.

Additional optimizations (copy propagation, loop-invariant code motion,
vectorization) are deferred to future versions.

### 6.4 Object Layout

Reference-type objects have the following runtime memory layout:

```
+------------------+
| strong_count: u32|    (non-atomic in v1; single-threaded)
+------------------+
| weak_count: u32  |
+------------------+
| type_tag: u32    |    (for dynamic dispatch / enum tag)
+------------------+
| padding          |
+------------------+
| field_0          |    (inline data starts here)
| field_1          |
| ...              |
+------------------+
```

The header is 16 bytes (4 + 4 + 4 + 4 padding), followed by the object's
fields laid out according to the type's field order with appropriate alignment.

### 6.5 Weak References

Weak references use a **side table** approach:

- Each ARC'd object has an inline `weak_count` in its header.
- When `weak_count > 0` and `strong_count` reaches 0, the object's fields are
  destroyed but the header is kept alive (so weak references can observe the
  zero strong count).
- When `weak_count` also reaches 0, the header is freed.
- `WeakUpgrade` checks `strong_count > 0`; if true, it increments `strong_count`
  and returns `Some(strong_ref)`; otherwise returns `None`.

### 6.6 ARC Counter Overflow

If the strong or weak reference count reaches `u32::MAX`, the runtime function
(`__nudl_arc_retain` or `__nudl_arc_weak_retain`) immediately aborts the
process with the diagnostic: "ARC reference count overflow". No unwinding
occurs — defer blocks and Drop implementations are not executed. This prevents
silent wraparound of the counter.

### 6.7 Field Drop Order

When an object's strong count reaches zero and its `drop` method (if any) has
executed, the object's fields are released in **declaration order** (first
field first). This is a deterministic, predictable order that matches the
struct definition.

For bindings within a scope, drop order is **reverse declaration order** (LIFO)
— the last binding declared is dropped first. `defer` blocks execute before
drops at scope exit.

When an enum value is dropped, the fields of the **active variant** are dropped
in declaration order, consistent with struct field drop behavior. Inactive
variants hold no data and require no drop operations.

### 6.8 dyn Interface Vtable Layout

A `dyn Interface` value is represented as a **fat pointer**: two machine words
(16 bytes on 64-bit platforms).

```
+-------------------+
| data_ptr: *void   |   -> ARC-managed object (same as a regular reference)
+-------------------+
| vtable_ptr: *void |   -> vtable for this (concrete type, interface) pair
+-------------------+
```

The vtable is a static, read-only table generated at compile time for each
(concrete type, interface) pair. Its layout:

```
+-------------------+
| method_0: fn ptr  |   first interface method (declaration order)
| method_1: fn ptr  |   second interface method
| ...               |
| method_N: fn ptr  |   last interface method
+-------------------+
| drop_fn: fn ptr   |   destructor for the concrete type
+-------------------+
| size: u64         |   size of the concrete type in bytes
+-------------------+
```

Method pointers are ordered by declaration order in the interface definition.
The `drop_fn` is used when the `dyn` value's reference count reaches zero.
The `size` field enables the runtime to know the allocation size for
deallocation.

### 6.9 Compile-Time Cycle Detection

The BC generator builds a **field reference graph** where nodes are types and
edges are reference-type fields. A cycle in this graph indicates a potential
reference cycle at runtime. When detected, the compiler emits a warning
suggesting the use of `weak` on one of the back-edges:

```
warning[W0301]: potential reference cycle detected
  --> src/main.nudl:5:5
   |
5  |     parent: Option<Node>,
   |     ^^^^^^
   |
   = note: Node.children -> Node[] -> Node.parent -> Node
   = help: consider using `weak` for the `parent` field:
           parent: Option<weak Node>,
```

---

## 7. Native Backend (`nudl-backend-llvm`)

### 7.1 Pipeline

The LLVM backend takes typed SSA bytecode and produces native executables
through LLVM:

```
SSA Bytecode
     |
     v
LLVM IR Generation       (SSA instruction -> LLVM IR via Inkwell)
     |
     v
LLVM Optimization        (LLVM's optimization passes)
     |
     v
Object File Emission     (LLVM TargetMachine -> .o file)
     |
     v
Linking                  (system linker + ARC runtime -> executable)
```

The backend uses the **Inkwell** library, which provides safe Rust bindings to
the LLVM 18 C API. This approach delegates instruction selection, register
allocation, and platform-specific codegen to LLVM, supporting multiple
architectures (ARM64, x86-64) and platforms (macOS, Linux) from a single
backend crate.

### 7.2 SSA to LLVM IR Translation

Each SSA instruction is lowered to LLVM IR instructions:

| SSA Instruction | LLVM IR Lowering |
|---|---|
| `Const(value)` | `LLVMConstInt` / `LLVMConstReal` / global string constant |
| `Add(dst, lhs, rhs)` [integer] | `build_int_add` |
| `Add(dst, lhs, rhs)` [float] | `build_float_add` |
| `Lt(dst, lhs, rhs)` [integer] | `build_int_compare(SLT)` |
| `Call(dst, func, args)` | `build_call` with ABI-conformant argument setup |
| `Retain(src)` | `build_call(__nudl_arc_retain)` with inline fast path |
| `Release(src)` | `build_call(__nudl_arc_release)` with inline fast path |
| `Load(dst, ptr, field)` | `build_struct_gep` + `build_load` |
| `Store(ptr, field, src)` | `build_struct_gep` + `build_store` |
| `Alloc(type)` | `build_call(malloc)` + header initialization |
| `Branch(cond, t, f)` | `build_conditional_branch` |

### 7.3 String Parameter Expansion

String values in nudl are reference-counted `(ptr, len)` pairs. In the LLVM
function signatures, string parameters are expanded to two parameters:
a pointer (`i8*`) and a length (`i64`). This avoids the need for struct passing
and matches common C string conventions for FFI interoperability.

### 7.4 ARC Runtime Integration

The backend compiles the C runtime (`runtime/nudl_rt.c`) at build time and links
it into the output binary. The runtime provides:

- **ARC operations:** `__nudl_arc_alloc`, `__nudl_arc_release_slow`,
  `__nudl_arc_overflow_abort`, weak reference operations
- **Dynamic arrays:** `__nudl_dynarray_alloc`, `push`, `pop`, `get`, `set`, `len`
- **Maps:** hash table with open-addressing and linear probing —
  `__nudl_map_alloc`, `insert`, `get`, `contains`, `remove`, `len`
- **Closures:** `__nudl_closure_env_alloc` for capture environment allocation

The backend also inlines fast paths for retain/release directly in LLVM IR:
the common case (non-null pointer, refcount > 1) avoids a function call.

### 7.5 Debug Symbols

The backend generates DWARF debug information via LLVM's debug info builder,
allowing source-level debugging with standard tools like `lldb`.

### 7.6 Target Platform Support

Because LLVM handles the platform-specific codegen, nudl supports any target
that LLVM supports. Current tested targets:

| Target | Status |
|--------|--------|
| `aarch64-apple-darwin` | Working |
| `x86_64-apple-darwin` | Working |
| `aarch64-linux-gnu` | Working |
| `x86_64-linux-gnu` | Working |

### 7.7 Diagnostic Flags

The CLI supports `--dump-llvm-ir` to print the generated LLVM IR and
`--dump-asm` to print the native assembly (via LLVM's target machine).

---
