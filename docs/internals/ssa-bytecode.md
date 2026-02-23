## 3. SSA Bytecode Design

### 3.1 Core Principles

The SSA bytecode is the central representation shared between the type checker,
the comptime VM, and the native backend. Design goals:

- **Typed.** Every register and instruction carries its nudl type. This
  information propagates directly from the type checker and is used by both
  the VM (for correct value representation) and the native backend (for
  register sizing and instruction selection).
- **SSA.** Every register is defined exactly once. Mutable variables from the
  source language are lowered to sequences of definitions with phi nodes at
  join points.
- **Flat.** No nested expressions. Every operation takes register operands and
  produces a register result. This makes analysis and codegen straightforward.
- **Shared.** The same bytecode is consumed by `nudl-vm` (interpretation) and
  `nudl-backend-arm64` (native compilation). No separate IR for comptime vs
  runtime.

### 3.2 Structure

A compiled program is a collection of `Function` definitions. Each function
contains:

```
Function
  name:         Symbol
  params:       [(Register, Type)]
  return_type:  Type
  is_async:     bool
  blocks:       [BasicBlock]
  register_map: Map<Register, Type>

BasicBlock
  id:           BlockId
  params:       [(Register, Type)]    // phi node parameters
  instructions: [Instruction]
  terminator:   Terminator
```

Basic blocks have explicit parameters instead of traditional phi nodes. When
a predecessor jumps to a block, it passes values for those parameters. This
is equivalent to phi nodes but makes the data flow explicit at the edge rather
than at the join.

### 3.3 Instruction Set

Instructions are organized by category. Most instructions write to exactly one
destination register (SSA property). Side-effect-only instructions (`Store`,
`IndexStore`, `Retain`, `Release`, `WeakRetain`, `WeakRelease`, `CancelTask`,
`Nop`) do not produce a result value.

```
// Arithmetic
Add(dst, lhs, rhs)        // dst = lhs + rhs
Sub(dst, lhs, rhs)        // dst = lhs - rhs
Mul(dst, lhs, rhs)        // dst = lhs * rhs
Div(dst, lhs, rhs)        // dst = lhs / rhs
Mod(dst, lhs, rhs)        // dst = lhs % rhs
Neg(dst, src)              // dst = -src

// Bitwise
BitAnd(dst, lhs, rhs)     // dst = lhs & rhs
BitOr(dst, lhs, rhs)      // dst = lhs | rhs
BitXor(dst, lhs, rhs)     // dst = lhs ^ rhs
Shl(dst, lhs, rhs)        // dst = lhs << rhs
Shr(dst, lhs, rhs)        // dst = lhs >> rhs
BitNot(dst, src)           // dst = ~src

// Comparison
Eq(dst, lhs, rhs)         // dst = lhs == rhs
Ne(dst, lhs, rhs)         // dst = lhs != rhs
Lt(dst, lhs, rhs)         // dst = lhs < rhs
Le(dst, lhs, rhs)         // dst = lhs <= rhs
Gt(dst, lhs, rhs)         // dst = lhs > rhs
Ge(dst, lhs, rhs)         // dst = lhs >= rhs

// Logical
Not(dst, src)              // dst = !src

// Constants
Const(dst, value)          // dst = immediate value
ConstUnit(dst)             // dst = ()

// Memory
Alloc(dst, type)           // allocate a reference-type object on the heap
Load(dst, ptr, field)      // dst = ptr.field (implicitly retains loaded reference)
Store(ptr, field, src)     // ptr.field = src   (no dst -- side effect only)
IndexLoad(dst, arr, idx)   // dst = arr[idx]
IndexStore(arr, idx, src)  // arr[idx] = src

// ARC
Retain(src)                // increment reference count of src
Release(src)               // decrement reference count; free if zero
WeakRetain(src)            // increment weak count
WeakRelease(src)           // decrement weak count
WeakUpgrade(dst, src)      // dst = Option::Some(strong) or Option::None

// Function calls
Call(dst, func, [args])    // dst = func(args...)
CallVirtual(dst, obj, method_idx, [args])  // dynamic dispatch through vtable

// Type operations
Cast(dst, src, type)       // dst = src as type
TypeCheck(dst, src, type)  // dst = src is type (bool)

// Tuples and aggregates
TupleCreate(dst, [elems])         // dst = (elems...)
TupleExtract(dst, tuple, index)   // dst = tuple.index
StructCreate(dst, type, [fields]) // dst = Type { fields... }
EnumCreate(dst, type, variant, [data])  // dst = Type::Variant(data...)
EnumTag(dst, src)                 // dst = discriminant of enum value
EnumData(dst, src, variant)       // dst = payload of enum variant

// Closures
ClosureCreate(dst, func, [captures])  // dst = closure capturing values
ClosureCall(dst, closure, [args])     // dst = closure(args...)

// Async
CreateFuture(dst, func, [captures])  // dst = future wrapping an async fn/block
Suspend(dst, future)                  // dst = await future (yields to executor)
Resume(dst, state)                    // resume suspended coroutine from state
SpawnTask(dst, future)                // dst = TaskHandle, schedule future on executor
CancelTask(handle)                    // request cancellation of spawned task

// Misc
Copy(dst, src)             // dst = src (value-type copy)
Nop                        // no operation (placeholder)
```

**Terminators** end a basic block and describe control flow:

```
Return(reg)                         // return reg from function
Jump(block, [args])                 // unconditional jump with block params
Branch(cond, true_block, [t_args],
             false_block, [f_args]) // conditional branch
Switch(reg, [(value, block, args)],
             default_block, d_args) // multi-way branch (for match)
Yield(future, resume_block, [args]) // suspend task, resume at resume_block when future resolves
Unreachable                         // marks dead code
```

### 3.4 Example: Source to SSA Bytecode

Consider this simple function:

```nudl
fn abs(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}
```

After type checking and SSA lowering:

```
function abs(r0: i32) -> i32:

  block entry(r0: i32):
    r1 = Const(0: i32)
    r2 = Lt(r0, r1)
    Branch(r2, then[], else[])

  block then():
    r3 = Neg(r0)
    Jump(merge[r3])

  block else():
    Jump(merge[r0])

  block merge(r4: i32):
    Return(r4)
```

The value `r4` in the `merge` block is a block parameter -- equivalent to a
phi node selecting between `r3` (from `then`) and `r0` (from `else`).

### 3.5 Example: ARC Operations

Reference-type assignments produce retain/release instructions:

```nudl
fn swap_field(mut obj: Point, new_child: Node) {
    let old = obj.child;
    obj.child = new_child;
    // old goes out of scope here
}
```

SSA bytecode:

```
function swap_field(r0: Point, r1: Node) -> ():

  block entry(r0: Point, r1: Node):
    r2 = Load(r0, "child")    // r2 = obj.child (Load implicitly retains)
    Retain(r1)                 // new_child gains a reference from the field
    Release(r2)                // old child loses the field reference
    Store(r0, "child", r1)     // obj.child = new_child
    Release(r2)                // local binding goes out of scope
    r3 = ConstUnit()
    Return(r3)
```

---
