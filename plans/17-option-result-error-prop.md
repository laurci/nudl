# Task 17: Option, Result, and Error Propagation (`?`)

## Goal
Implement built-in `Option<T>` and `Result<T, E>` enum types plus the `?` operator for ergonomic error propagation.

## Requirements

### Option Type
- `Option<T>` with variants `Some(T)` and `None`
- Built-in (compiler-known) — not defined in user code
- Add `TypeKind::Option(TypeId)` or reuse enum infrastructure from Task 11
- Constructors: `Some(value)`, `None`
- Pattern matching: `match opt { Some(x) => ..., None => ... }`

### Result Type
- `Result<T, E>` with variants `Ok(T)` and `Err(E)`
- Built-in (compiler-known)
- Add `TypeKind::Result(TypeId, TypeId)` or reuse enum infrastructure
- Constructors: `Ok(value)`, `Err(error)`
- Pattern matching: `match res { Ok(x) => ..., Err(e) => ... }`

### Error Propagation (`?`)
- `expr?` on `Option<T>`: if `None`, return `None` from enclosing function; if `Some(x)`, evaluate to `x`
- `expr?` on `Result<T, E>`: if `Err(e)`, return `Err(e)` from enclosing function; if `Ok(x)`, evaluate to `x`
- Enclosing function must return `Option<_>` or `Result<_, E>` respectively
- Parse `?` as a postfix operator

### Optional Parameters
- `fn f(x?: i32)` — parameter type is `Option<i32>`, caller can omit it (defaults to `None`)
- Sugar for `fn f(x: Option<i32> = None)`

### Desugaring
- `expr?` on Option → `match expr { Some(__val) => __val, None => return None }`
- `expr?` on Result → `match expr { Ok(__val) => __val, Err(__e) => return Err(__e) }`

## Acceptance Criteria
- `tests/error-handling/option.nudl` compiles and runs
- `tests/error-handling/result.nudl` compiles and runs
- `tests/error-handling/question_mark.nudl` compiles and runs
- `tests/functions/optional_params.nudl` compiles and runs
- `Some(42)` creates an Option with value
- `None` creates empty Option
- `?` on Option early-returns None
- `?` on Result early-returns Err
- Pattern matching on Option and Result works
- Type error when using `?` in function not returning Option/Result

## Technical Notes
- Option and Result can be implemented as regular enums using the Task 11 infrastructure, but with special compiler knowledge for the `?` operator
- The `?` operator desugaring happens during lowering (after type checking validates the types)
- The token for `?` already exists in the lexer (STATUS.md says "token exists, not parsed")
- Depends on: Task 11 (enums), Task 12 (pattern matching) for the underlying infrastructure
- Option/Result could be defined as built-in types in the type interner (always available)
- Consider: should these be actual enums in the IR or have special-case codegen? Using the enum infra is cleaner
