# Task 24: Destructuring

## Goal
Implement let-binding destructuring for tuples and structs, enabling `let (x, y) = get_point();` and `let Foo { x, y } = make_foo();`.

## Requirements

### Tuple Destructuring
- `let (a, b, c) = tuple_expr;`
- `let (a, _, c) = tuple_expr;` — wildcard for unused elements
- `let (a, ..) = tuple_expr;` — rest pattern (ignore remaining) — maybe post-MVP
- Mutable: `let mut (a, b) = ...;` makes all bindings mutable
- Nested: `let ((a, b), c) = nested_tuple;`

### Struct Destructuring
- `let Foo { x, y } = foo_expr;`
- `let Foo { x, y: renamed } = foo_expr;` — rename fields
- `let Foo { x, .. } = foo_expr;` — ignore remaining fields
- Nested: `let Foo { point: (x, y) } = foo_with_tuple_field;`

### Function Parameter Destructuring
- `fn f((x, y): (i32, i32)) -> i32 { x + y }`
- `fn f(Foo { x, y }: Foo) -> i32 { x + y }`

### Type Checking
- Validate pattern matches the type of the initializer
- Each binding gets the type of the corresponding element/field
- Wildcard `_` discards the value (but still type-checks)

### Lowering
- Tuple destructuring: lower to individual element accesses
  - `let (a, b) = t;` → `let a = t.0; let b = t.1;`
- Struct destructuring: lower to individual field accesses
  - `let Foo { x, y } = f;` → `let x = f.x; let y = f.y;`
- No new IR instructions — reuse existing field/element access

## Acceptance Criteria
- `tests/variables/destructuring.nudl` compiles and runs
- Tuple destructuring in let bindings
- Struct destructuring in let bindings
- Nested destructuring
- Wildcard in patterns
- Function parameter destructuring
- Type errors for mismatched patterns

## Technical Notes
- This reuses the Pattern AST from Task 12 (pattern matching) — let binding patterns are a subset
- `let pattern = expr;` is essentially an irrefutable pattern match
- For refutable patterns (e.g., enum), `let` should error — use `if let` instead
- The lowering is straightforward: extract elements and create individual bindings
- Depends on: Task 06 (tuples), Task 12 (pattern matching infrastructure)
- ARC consideration: destructuring a struct should retain each extracted field
