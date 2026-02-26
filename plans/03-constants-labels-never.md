# Task 03: Constants, Labeled Loops, Never Type

## Goal
Implement `const` declarations (module-level and local), labeled loops (`'label: loop { break 'label; }`), and the never type (`!`).

## Requirements

### Constants
- Parse `const NAME: Type = expr;` at module level and inside function bodies
- Constants must have a type annotation
- Value must be a compile-time evaluable expression (literals, const arithmetic, other constants)
- Type checker: validate constness, register in scope
- Lowerer: inline constant values at usage sites (no runtime storage needed)
- Constants are always immutable

### Labeled Loops
- Parse `'label: loop { ... }`, `'label: while cond { ... }`
- Parse `break 'label` and `continue 'label`
- Type checker: track label scopes, validate label references
- Lowerer: use label names to resolve which loop's break/continue blocks to jump to
- Labels are lexically scoped — inner loops can shadow outer labels (with a warning)

### Never Type
- Add `TypeKind::Never` to the type system
- Never is a subtype of all types (coerces to anything)
- `break`, `continue`, `return`, and infinite loops have type `!`
- Useful for match arms that diverge: `match x { ... _ => panic("unreachable") }`
- No runtime representation needed

## Acceptance Criteria
- `tests/variables/constants.nudl` compiles and runs
- `tests/control-flow/labeled_loops.nudl` compiles and runs
- `const X: i32 = 42;` followed by `print(X)` prints 42
- `break 'outer` from a nested loop exits the correct loop
- Never type unifies with any other type without errors

## Technical Notes
- Parser: `const` is likely already a reserved keyword/token — wire it into item parsing
- For labels, the lexer needs to handle `'identifier` as a lifetime/label token (Rust-style)
- The IR doesn't need a special const instruction — just resolve to `ConstValue` during lowering
- Labeled break/continue: the lowerer already tracks loop contexts for break/continue; extend with label map
- Never type: primarily a type-checker concern; the IR never materializes `!` values
