# Task 20: Trailing Lambdas

## Goal
Implement trailing lambda syntax where a closure can be passed as the last argument outside parentheses, and single-parameter closures get an implicit `it` parameter.

## Requirements

### Trailing Lambda Syntax
- `f(args) { body }` → desugars to `f(args, |...| { body })`
- The trailing block is the last argument to the function call
- Works with any function whose last parameter is a function type

### Implicit `it` Parameter
- When a trailing lambda has no explicit parameters: `f() { it + 1 }`
- Desugars to `f(|it| { it + 1 })`
- `it` type is inferred from the expected function type's parameter
- Only works for single-parameter function types

### Parsing
- After parsing a function call `f(args)`, check if next token is `{`
- If so, parse the block as a closure body
- If no `|params|` is present inside, use implicit `it`
- Also works with no call parens: `f { body }` → `f(|it| { body })`

### Type Checking
- Resolve the trailing lambda's parameter types from the expected function parameter type
- Validate the trailing block body matches the expected return type

## Acceptance Criteria
- `tests/functions/trailing_lambda.nudl` compiles and runs
- `apply(5) { it * 2 }` → calls `apply(5, |it| it * 2)`
- `items.each { print(it); }` → calls `items.each(|it| print(it))`
- Explicit params: `items.fold(0) { |acc, x| acc + x }`
- Type inference for `it` works correctly
- Error when trailing lambda doesn't match expected function type

## Technical Notes
- This is purely syntactic sugar — desugar to closure before type checking
- The parser needs to look ahead after `)` for `{`
- Depends on: Task 19 (closures) for the underlying closure mechanism
- Consider: does `f { }` (no parens) work? If so, it means function call with single closure arg
- The `it` keyword could be a regular identifier that's only special inside trailing lambdas, or a true keyword
- For MVP, making `it` a regular identifier used as default closure param is simpler
