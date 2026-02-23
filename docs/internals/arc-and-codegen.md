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

## 7. Native Backend (`nudl-backend-arm64`)

### 7.1 Pipeline

The ARM64 backend takes typed SSA bytecode and produces machine code:

```
SSA Bytecode
     |
     v
Instruction Selection    (SSA instruction -> ARM64 instruction sequence)
     |
     v
Register Allocation      (virtual registers -> physical ARM64 registers)
     |
     v
Prologue/Epilogue        (stack frame setup, callee-saved register saves)
     |
     v
Encoding                 (ARM64 instructions -> bytes)
     |
     v
Machine Code Buffer      (raw bytes + relocation records)
```

### 7.2 Instruction Selection

Each SSA instruction is lowered to one or more ARM64 instructions. The selector
handles type-specific lowering:

| SSA Instruction | ARM64 Lowering |
|---|---|
| `Add(dst, lhs, rhs)` [i32/i64] | `ADD Xd, Xn, Xm` |
| `Add(dst, lhs, rhs)` [f64] | `FADD Dd, Dn, Dm` |
| `Lt(dst, lhs, rhs)` [i32] | `CMP Wn, Wm` + `CSET Wd, LT` |
| `Call(dst, func, args)` | argument setup + `BL func` + result move |
| `Retain(src)` | `BL __nudl_arc_retain` |
| `Release(src)` | `BL __nudl_arc_release` |
| `Load(dst, ptr, field)` | `LDR Xd, [Xn, #offset]` |
| `Store(ptr, field, src)` | `STR Xm, [Xn, #offset]` |
| `Branch(cond, t, f)` | `CBNZ/CBZ` or `B.cond` |

### 7.3 Register Allocation

The backend uses **linear scan register allocation** over live intervals
computed from the SSA form:

1. Compute live intervals for each virtual register using a reverse pass over
   the basic blocks.
2. Sort intervals by start position.
3. Walk intervals in order, assigning physical registers from the free pool.
4. When no registers are free, spill the interval with the furthest next use
   to the stack.

ARM64 provides 31 general-purpose registers (X0-X30) and 32 SIMD/FP registers
(V0-V31). The allocator reserves:

- **X0-X7:** argument/return registers (per Apple ARM64 ABI)
- **X8:** indirect result register
- **X16-X17:** intra-procedure-call scratch (linker veneers)
- **X18:** platform reserved (macOS)
- **X29:** frame pointer
- **X30:** link register
- **SP:** stack pointer

Registers X9-X15 and X19-X28 are available for allocation, with X19-X28 being
callee-saved.

### 7.4 Calling Convention

nudl follows the **Apple ARM64 ABI** for interoperability:

- First 8 integer/pointer arguments in X0-X7.
- First 8 floating-point arguments in V0-V7 (D0-D7 for f64).
- Additional arguments spilled to the stack.
- Return value in X0 (integer/pointer) or V0 (float).
- Callee saves X19-X28, V8-V15.

Function prologue/epilogue:

```asm
; Prologue
STP X29, X30, [SP, #-frame_size]!    ; save frame pointer and link register
MOV X29, SP                           ; establish frame pointer
STP X19, X20, [SP, #16]              ; save callee-saved registers (as needed)
; ... function body ...

; Epilogue
LDP X19, X20, [SP, #16]              ; restore callee-saved registers
LDP X29, X30, [SP], #frame_size      ; restore frame pointer and link register
RET                                    ; return via X30
```

### 7.5 ARC Runtime Functions

The backend emits calls to a small runtime library for ARC operations:

```
__nudl_arc_retain(ptr):
    if ptr != null:
        ptr->strong_count += 1

__nudl_arc_release(ptr):
    if ptr != null:
        ptr->strong_count -= 1
        if ptr->strong_count == 0:
            call ptr->drop_fn(ptr)      // destroy fields
            if ptr->weak_count == 0:
                free(ptr)               // deallocate
            // else: header stays for weak refs

__nudl_arc_weak_upgrade(ptr) -> Option<ptr>:
    if ptr->strong_count == 0:
        return None
    ptr->strong_count += 1
    return Some(ptr)
```

---
