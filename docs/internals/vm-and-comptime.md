## 4. Virtual Machine (`nudl-vm`)

### 4.1 Architecture

The nudl VM is a **register-based interpreter** that executes SSA bytecode.
Register-based design is a natural fit for SSA form: each SSA register maps
directly to a VM register slot, avoiding the push/pop overhead of a
stack-based VM.

```
+------------------------------------------+
|  VM Instance                             |
|                                          |
|  +------------------------------------+  |
|  | Call Stack                         |  |
|  |  Frame 0: main()                  |  |
|  |    registers: [r0, r1, r2, ...]   |  |
|  |    current_block: entry            |  |
|  |    ip: 3                           |  |
|  |  Frame 1: helper()                |  |
|  |    registers: [r0, r1, ...]        |  |
|  |    current_block: loop_body        |  |
|  |    ip: 1                           |  |
|  +------------------------------------+  |
|                                          |
|  +------------------------------------+  |
|  | Heap (ARC'd objects)              |  |
|  |  0x100: Point { x: 1.0, y: 2.0 } |  |
|  |         refcount: 2, weak: 0      |  |
|  |  0x200: Node[] [...]              |  |
|  |         refcount: 1, weak: 1      |  |
|  +------------------------------------+  |
|                                          |
|  step_counter: 14823                     |
|  step_limit:   1000000                   |
+------------------------------------------+
```

### 4.2 Value Representation

VM values are tagged unions that carry their runtime type:

```
Value
  |- I8(i8) | I16(i16) | I32(i32) | I64(i64)
  |- U8(u8) | U16(u16) | U32(u32) | U64(u64)
  |- F32(f32) | F64(f64)
  |- Bool(bool)
  |- Char(char)
  |- Unit
  |- String(Rc<String>)
  |- Tuple(Vec<Value>)
  |- Array(Rc<RefCell<Vec<Value>>>)
  |- FixedArray(Vec<Value>)
  |- Struct(Rc<RefCell<StructObj>>)
  |- Enum(Rc<RefCell<EnumObj>>)
  |- Closure(Rc<ClosureObj>)
  |- WeakRef(Weak<RefCell<...>>)
  |- Future(Rc<FutureObj>)  // async computation (runtime only)
  |- Actor(Rc<RefCell<ActorObj>>)  // actor instance (runtime only)
  |- Type(TypeId)           // first-class type value (comptime only)
  |- AstFragment(AstNode)   // code-as-data (comptime only)
```

Value types (`I32`, `Bool`, `Tuple` of value types, `FixedArray`) are copied
on assignment. Reference types (`String`, `Array`, `Struct`, `Enum`,
`Closure`) use Rust's `Rc<RefCell<...>>` for ARC semantics within the VM.

### 4.3 Execution Loop

The VM's core loop is a fetch-decode-execute cycle over SSA instructions:

```
fn execute(vm, function) -> Value:
    push call frame for function
    loop:
        increment step_counter
        if step_counter > step_limit:
            error("comptime evaluation exceeded step limit")

        instruction = current_block.instructions[ip]
        match instruction:
            Add(dst, lhs, rhs) =>
                vm.regs[dst] = vm.regs[lhs].add(vm.regs[rhs])
            Call(dst, func, args) =>
                push new call frame
                continue (recursive execution)
            Return(reg) =>
                result = vm.regs[reg]
                pop call frame
                if call stack empty: return result
                write result to caller's dst register
            Branch(cond, t_block, f_block) =>
                if vm.regs[cond].as_bool():
                    jump to t_block with args
                else:
                    jump to f_block with args
            ...
        ip += 1
```

### 4.4 Step Limit Enforcement

The VM enforces a configurable step limit to guarantee termination of comptime
evaluation. Each instruction execution increments a counter. If the counter
exceeds the limit, the VM halts with a diagnostic that includes the call stack
at the point of termination:

```
error[E0501]: comptime evaluation exceeded step limit (1000000)
  --> src/main.nudl:12:5
   |
12 |     comptime { infinite_loop(); }
   |     ^^^^^^^^
   |
   = note: call stack at termination:
           infinite_loop (src/lib.nudl:42:9)
           helper (src/lib.nudl:38:5)
```

### 4.5 VM ARC Runtime

Reference types within the VM use Rust's `Rc`/`Weak` to model nudl's ARC
semantics. When the VM executes a `Retain` instruction, it clones the `Rc`.
When it executes `Release`, it drops the `Rc`. Rust's built-in reference
counting handles the actual deallocation.

This means the VM faithfully models the runtime behavior of ARC, including
detecting cycles that would leak (the same cycles that would leak in compiled
code). However, since comptime execution is short-lived and heap allocations
cannot escape, cycle leaks in comptime are bounded and cleaned up when the VM
instance is dropped.

---

## 5. Comptime Execution Model

### 5.1 Pipeline

The comptime execution model creates a feedback loop in the compilation
pipeline:

```
     Source
       |
       v
   Parse (AST)
       |
       v
  Type Check + BC Gen  <-----------+
       |                            |
       v                            |
   Identify comptime blocks         |
       |                            |
       v                            |
   Execute in VM  -------->  New AST fragments
       |                     (structs, functions,
       v                      impl blocks)
   Comptime values
   (constants for
    runtime code)
```

Steps in detail:

1. The parser identifies `comptime { ... }` blocks and `comptime fn`
   definitions and marks them in the AST.
2. The type checker processes comptime items. Comptime functions have the same
   type rules as runtime functions but may additionally accept `type` values as
   parameters and return `type` values or `AstFragment`s.
3. The BC generator produces SSA bytecode for comptime blocks.
4. The VM executes the comptime bytecode.
5. If the VM evaluates `quote` blocks, it produces AST fragments.
6. Those AST fragments are injected back into the compilation pipeline at the
   type checking stage, as if they had been written by the programmer.
7. The newly generated code goes through type checking and BC generation
   normally.
8. Steps 4-7 may repeat if generated code itself contains comptime blocks
   (with a recursion depth limit).

### 5.2 Comptime Intrinsics and Quote

Code generation uses `quote { ... }` blocks that produce AST fragments.
Inside `quote`, `${}` splices in comptime values. The VM also provides
built-in reflection functions (no special prefix):

| Function | Description |
|---|---|
| `type_name(T)` | Returns the name of type `T` as a string |
| `type_fields(T)` | Returns the field names and types of a struct |
| `type_variants(T)` | Returns the variant names and payloads of an enum |
| `type_methods(T)` | Returns method signatures of a type |
| `type_implements(T, I)` | Checks whether type `T` implements interface `I` |
| `call_site()` | Returns source location information about the caller |
| `size_of(T)` | Returns the size of type `T` in bytes |
| `align_of(T)` | Returns the alignment of type `T` in bytes |
| `ast_of(item)` | Returns read-only AST of an existing item |
| `attributes(item)` | Returns attributes attached to an item |
| `field_attributes(T, name)` | Returns attributes on a specific struct field |
| `module_types()` | All struct/enum types defined in current module |
| `module_functions()` | All functions defined in current module |
| `module_interfaces()` | All interfaces defined in current module |
| `imported_types()` | Types explicitly imported into current scope |

`quote` blocks are parsed into AST at compile time, with `${}` holes
resolved from the comptime environment. The resulting AST fragments are
injected back into the compilation pipeline at the type-checking stage.

### 5.3 Types as First-Class Values

At comptime, types are values of the special `type` kind. You can store them in
variables, pass them to functions, put them in arrays, and compare them:

```nudl
comptime fn numeric_types() -> type[] {
    [i8, i16, i32, i64, u8, u16, u32, u64, f32, f64]
}

comptime fn make_conversions() {
    let types = numeric_types();
    for from in types {
        for to in types {
            if from != to {
                let name = `{type_name(from)}_to_{type_name(to)}`;
                quote {
                    fn ${name}(x: ${from}) -> ${to} {
                        x as ${to}
                    }
                }
            }
        }
    }
}
```

### 5.4 Comptime Value Serialization

When a comptime block produces a value used at runtime, that value must be
serialized into a constant in the SSA bytecode:

- **Primitive values** (`i32`, `f64`, `bool`, etc.) are embedded directly as
  `Const` instructions.
- **Strings** are placed in the program's read-only data section and referenced
  by pointer.
- **Arrays of constants** are emitted as static data.
- **Types** cannot escape to runtime -- using a `type` value in a runtime
  context is a compile error.

### 5.5 Comptime Restrictions

Comptime execution is sandboxed:

- **No I/O.** No file access, no network, no printing (except via
  `comptime_print` for debugging, which emits a compiler note).
- **No heap escape.** Reference-type values allocated during comptime cannot
  be returned to runtime code. Only value types and serializable constants
  can cross the boundary.
- **Step limit.** A configurable maximum number of VM instructions prevents
  infinite loops.
- **Recursion depth limit.** Comptime code that emits more comptime code has a
  bounded recursion depth (default: 16).

### 5.6 Async State Machine Lowering

Each `async fn` is compiled into a state machine at the SSA bytecode level.
The lowering process transforms suspension points (`.await` expressions) into
state transitions.

**Transformation overview:**

1. **State enumeration.** Each suspension point (`.await`) in the function
   body creates a new state variant. An `async fn` with N await points
   produces N+1 states (initial + one per await).

2. **State struct.** All local variables that are live across a suspension
   point are stored in a heap-allocated state struct. Variables that do not
   span suspension points remain in SSA registers.

   ```
   // Source:
   async fn example() -> i32 {
       let a = compute_a().await;    // suspend point 0
       let b = compute_b().await;    // suspend point 1
       a + b
   }

   // Generates state struct:
   struct ExampleState {
       state_tag: u8,    // 0 = initial, 1 = after first await, 2 = after second
       a: i32,           // lives across suspend point 1
   }
   ```

3. **Poll function.** The state machine is compiled into a poll function that
   resumes execution from the current state and runs until the next suspension
   point or completion:

   ```
   function example_poll(state: ExampleState) -> PollResult<i32>:
     block entry(state):
       Switch(state.state_tag,
         [(0, state_0, [state]),
          (1, state_1, [state]),
          (2, state_2, [state])],
         unreachable)

     block state_0(state):
       r0 = Call(compute_a, [])     // returns Future<i32>
       Yield(r0, state_1, [state])  // suspend, resume at state_1

     block state_1(state):
       r1 = Resume(state)           // r1 = result of compute_a
       state.a = r1
       r2 = Call(compute_b, [])
       Yield(r2, state_2, [state])

     block state_2(state):
       r3 = Resume(state)           // r3 = result of compute_b
       r4 = Add(state.a, r3)
       Return(r4)                   // poll complete
   ```

4. **Future wrapping.** A `CreateFuture` instruction wraps the state struct
   and poll function into a `Future<T>` value that the executor can drive.

### 5.7 Cooperative Executor

The runtime includes a single-threaded cooperative executor that drives async
tasks to completion.

**Architecture:**

```
+--------------------------------------+
|  Executor                            |
|                                      |
|  ready_queue:  [TaskId]              |
|  tasks:        Map<TaskId, Task>     |
|  current_task: TaskId                |
|                                      |
|  Task                                |
|    id:          TaskId               |
|    future:      Future<T>            |
|    state:       Ready | Suspended    |
|    cancelled:   bool                 |
|    children:    [TaskId]             |
|    parent:      Option<TaskId>       |
+--------------------------------------+
```

**Run loop:**

1. Pop a task from the ready queue.
2. Poll the task's future (call its poll function).
3. If the future yields (`Yield` terminator), the task is suspended. The
   awaited sub-future is checked:
   - If the sub-future is already complete, the task is immediately
     re-enqueued to the ready queue.
   - Otherwise, the task is parked until the sub-future completes.
4. If the future returns (`Return` terminator), the task is complete. Any
   parent task waiting on this result is enqueued to the ready queue.
5. Repeat until the ready queue is empty.

**Cooperative scheduling model:**

The executor uses a **cooperative, non-preemptive** scheduling model. A task
that never reaches an `.await` suspension point will run to completion, blocking
all other tasks. This is by design — it matches the single-threaded cooperative
model of JavaScript and Zig.

It is the programmer's responsibility to insert `.await` points in long-running
computations. The compiler does **not** warn about async functions with no await
points, nor does it insert implicit yield points.

**Structured concurrency enforcement:**

- When a scope exits (`Task.spawn` scope or `Task.group` block), the executor
  cancels all child tasks that are still running.
- Cancellation sets the `cancelled` flag on the task. At the next `.await`
  suspension point, the cancelled task's stack automatically unwinds: `defer`
  blocks execute in LIFO order, `Drop` implementations run normally for all
  values going out of scope, and the task is marked complete with a
  cancellation status. Between suspension points, tasks may check
  `Task.is_cancelled()` proactively.
- `Task.group` waits for all children to complete before returning results.

**Task cancellation result:**

When a cancelled task is awaited, the `TaskHandle<T>` resolves to
`Result<T, CancelledError>`. The caller can handle cancellation explicitly:

```nudl
let handle = Task.spawn(async { compute().await });
// ... later ...
handle.cancel();
match handle.await {
    Ok(value) => println(`completed: {value}`),
    Err(CancelledError) => println("task was cancelled"),
}
```

`CancelledError` is a built-in error type that implements the `Error`
interface. Within a `Task.group`, if any child task is cancelled, its result in
the collected results array is `Err(CancelledError)`.

**No async Drop:**

`Drop` implementations are always synchronous. Async operations cannot be
performed during drop. For resources that require async cleanup (e.g., closing
network connections, flushing async writers), the recommended pattern is an
explicit `close()` or `shutdown()` async method called before the value goes
out of scope, optionally enforced via `defer`:

```nudl
let conn = Connection::open(addr).await;
defer { conn.close().await; }
// ... use conn ...
```

### 5.8 Build Script VM Mode

Build scripts (`build.nudl`) execute in the comptime VM with additional host
functions registered. The VM creates a separate execution context for the build
script with:

- **Extended step limit** (default: 10,000,000 vs 1,000,000 for regular
  comptime).
- **File I/O host functions:** `build::read_file` is implemented as a VM
  host function that reads from the project directory. Path traversal outside
  the project root is rejected.
- **Write host function:** `build::generate_file` writes to `.nudl/generated/`
  within the project root.
- **Environment access:** `build::env` reads from the process environment.
- **Define registration:** `build::add_define` stores name-value pairs that
  are injected as comptime constants into the main compilation.

The build script VM instance is destroyed after execution. Its outputs (files
in `.nudl/generated/` and registered defines/flags) are consumed by the main
compilation pipeline.

---
