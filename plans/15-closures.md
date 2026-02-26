# Task 15: Closures

## Goal
Implement first-class function values and closures with capture analysis, heap-allocated capture environments, and ARC management.

## Requirements

### Parsing
- Closure syntax: `|params| body` or `|params| -> RetType { body }`
- Examples: `|x| x + 1`, `|x: i32, y: i32| -> i32 { x + y }`, `|| { print("hello"); }`
- Type annotation for closure params is optional (inferred from context)
- Closures as expressions: `let add = |x, y| x + y;`

### Function Types
- `TypeKind::Function` already exists — make it usable as a first-class value
- Function type syntax in annotations: `fn(i32, i32) -> i32` or `(i32, i32) -> i32`
- Closures and function references share the same type

### Capture Analysis
- Determine which variables from enclosing scope a closure references
- Capture by value (copy) for value types
- Capture by reference (retain) for reference types
- Mutable captures: if the closure mutates a captured variable, it must capture mutably

### Runtime Representation
- Closure = fat pointer: `{ fn_ptr: ptr, env_ptr: ptr }`
- `env_ptr` points to a heap-allocated capture struct (ARC'd)
- Capture struct layout: `{ ref_count: i64, captured_var1, captured_var2, ... }`
- If no captures, `env_ptr` is null (plain function pointer)

### IR
- `Instruction::ClosureAlloc { dst, function_id, captures: Vec<Reg> }`
- `Instruction::ClosureCall { dst, closure, args: Vec<Reg> }`
- `Instruction::CaptureLoad { dst, env, index }` — load from capture struct
- Retain/Release on the capture environment

### Type Checking
- Infer closure parameter types from usage context (or require annotations)
- Return type inferred from body
- Captured variables: check they exist in scope, validate mutability

### Codegen
- Generate a separate LLVM function for each closure body
- First parameter of closure function is `env_ptr`
- Closure call: extract fn_ptr and env_ptr, call fn_ptr with env_ptr prepended to args
- Capture struct: generate LLVM struct type per closure

## Acceptance Criteria
- `tests/functions/closures.nudl` compiles and runs
- `tests/core-types/function_types.nudl` compiles and runs
- Simple closure: `let f = |x: i32| x * 2; f(5)` → 10
- Capture: `let y = 10; let f = |x| x + y; f(5)` → 15
- Closure as function argument: `fn apply(f: fn(i32) -> i32, x: i32) -> i32 { f(x) }`
- Multiple captures work correctly
- ARC on capture environment (no leaks)
- Function references: `let f = some_function; f(args)`

## Technical Notes
- Capture analysis happens during type checking or lowering
- Each closure generates a unique function in the IR with an extra env parameter
- The env parameter is a pointer to the capture struct; closures with no captures pass null
- For calling conventions: closure functions take `(env_ptr, arg1, arg2, ...)`
- When a closure is called via function type, the caller always passes env_ptr (even if null)
- This means plain function references also need to be wrapped as closures with null env
- Depends on: ARC (for capture struct lifetime)
