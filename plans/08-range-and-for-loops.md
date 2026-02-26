# Task 08: Range Types and For Loops

## Goal
Implement range expressions (`0..10`, `0..=10`) and `for` loops over ranges and fixed arrays, enabling `for i in 0..10 { ... }` and `for item in array { ... }`.

## Requirements

### Range Types
- Add `TypeKind::Range(TypeId)` — range over integer types
- `a..b` creates exclusive range [a, b)
- `a..=b` creates inclusive range [a, b]
- Range is a value type (just two integers: start + end)
- Ranges are not indexable or iterable as standalone values in MVP — only used in for-loops

### For Loop Parsing
- `for pattern in expr { body }` — standard for-in syntax
- Pattern is a simple binding for MVP: `for i in ...` or `for item in ...`
- Expr must be a range or a fixed array (dynamic arrays added later)

### For Loop Desugaring (to while loop)
- `for i in start..end { body }` →
  ```
  let mut __iter = start;
  while __iter < end {
    let i = __iter;
    body;
    __iter = __iter + 1;
  }
  ```
- `for i in start..=end { body }` → same but `<=` comparison
- `for item in array { body }` →
  ```
  let mut __idx = 0;
  while __idx < len {
    let item = array[__idx];
    body;
    __idx = __idx + 1;
  }
  ```

### Type Checking
- Range operands must be the same integer type
- For-loop binding gets the element type (integer for ranges, T for `[T; N]`)
- Break and continue work inside for-loops

### IR
- No new IR instructions needed — desugared to existing while-loop IR
- Range itself doesn't need runtime representation if only used in for-loops

## Acceptance Criteria
- `tests/operators/range.nudl` compiles (range expressions parse correctly)
- `tests/control-flow/for_loops.nudl` compiles and runs
- `for i in 0..5 { print(i); }` prints 0,1,2,3,4
- `for i in 0..=5 { print(i); }` prints 0,1,2,3,4,5
- `for x in [1,2,3] { print(x); }` prints 1,2,3
- Break and continue work inside for-loops
- Nested for-loops work

## Technical Notes
- Desugaring should happen during AST→IR lowering (not in the parser)
- The range expressions `..` and `..=` are already parsed (STATUS.md says "parsed, not lowered")
- For-loop token already exists (STATUS.md says "token exists, not parsed")
- No Iterator interface needed for MVP — special-case ranges and arrays
- The desugaring creates internal variables (use name mangling like `__for_iter_0`)
- Depends on: Task 07 (fixed arrays) for array iteration, though range iteration is independent
