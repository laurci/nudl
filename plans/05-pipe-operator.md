# Task 05: Pipe Operator (`|>`)

## Goal
Implement the `|>` pipe operator that desugars `x |> f` to `f(x)` and `x |> f(y)` to `f(x, y)`, enabling fluent left-to-right function composition.

## Requirements

### Parsing
- Parse `|>` as a left-associative binary operator
- Precedence: below assignment, above nothing useful — it's the lowest-precedence "value" operator
- Actually, typical precedence: above assignment, below logical OR — check spec
- RHS must be either:
  - A function name: `x |> f` → `f(x)`
  - A function call: `x |> f(y, z)` → `f(x, y, z)` (LHS inserted as first argument)

### Desugaring
- This is a purely syntactic transformation — no new IR instructions needed
- Desugar in the parser or as an AST transformation before type checking
- Chain: `x |> f |> g(y)` → `g(f(x), y)`

### Type Checking
- After desugaring, normal function call type checking applies
- Error messages should reference the original pipe syntax for clarity

## Acceptance Criteria
- `tests/operators/pipe.nudl` compiles and runs correctly
- Simple pipe: `5 |> double` calls `double(5)`
- Pipe with args: `5 |> add(3)` calls `add(5, 3)`
- Chained pipes: `5 |> double |> add(1)` → `add(double(5), 1)`
- Type errors in pipe expressions have reasonable messages

## Technical Notes
- The `PipeRight` token (`|>`) already exists in the lexer
- Simplest implementation: desugar in the AST during or after parsing, before type checking
- Alternative: desugar during lowering, but AST-level is simpler and gives better error messages
- The parser needs to handle `|> ident` (no parens) as a special case vs `|> ident(args)`
- Consider: does `x |> f.method()` work? Probably not in MVP — just top-level function calls
