# Task 09: Named Arguments, Default Parameters, Argument Shorthand

## Goal
Enable calling functions with named arguments (`f(x: 42)`), defining default parameter values (`fn f(x: i32 = 0)`), and argument shorthand (`f(x)` as shorthand for `f(x: x)` when a local variable matches the parameter name).

## Requirements

### Named Arguments
- Call syntax: `f(name: value)` — matches arguments to parameters by name
- Can mix positional and named, but positional must come first
- All parameters after first named must also be named
- AST node for named args already has a field — just needs parser wiring
- Type checker: resolve named args to parameter positions, validate names exist

### Default Parameters
- Declaration: `fn f(x: i32, y: i32 = 10, z: bool = false) -> i32`
- Default values must be compile-time constants (literals, const exprs)
- Parameters with defaults must come after required parameters
- At call site, trailing defaulted params can be omitted
- Type checker: fill in default values for missing arguments

### Argument Shorthand
- If a function parameter is `name: Type` and a local variable `name` exists with compatible type:
  - `f(name)` is shorthand for `f(name: name)`
- Only works when calling with named arguments context
- This is syntactic sugar resolved during type checking

### Lowering
- Named args are reordered to positional during lowering
- Default values are inserted as constant args during lowering
- No new IR instructions needed

## Acceptance Criteria
- `tests/functions/named_arguments.nudl` compiles and runs
- `tests/functions/default_params.nudl` compiles and runs
- `tests/functions/argument_shorthand.nudl` compiles and runs
- `f(y: 20, x: 10)` correctly passes x=10, y=20
- `f(10)` with `fn f(x: i32, y: i32 = 5)` passes x=10, y=5
- Error when named arg doesn't match any parameter

## Technical Notes
- Parser: the named arg field in the AST already exists but parser "never sets it" per STATUS.md
- The parser needs to detect `ident: expr` in argument position (disambiguate from type annotation)
- Default values can be stored in the function AST node and carried through to type checking
- Consider: should default exprs be evaluated at call site or definition site? Definition site (compile-time constants) is simpler and correct for MVP
