# Task 12: Pattern Matching

## Goal
Implement `match` expressions, `if let`, and let destructuring with support for literal, binding, wildcard, tuple, struct, and enum patterns. Include exhaustiveness checking.

## Requirements

### Pattern Types
- **Wildcard**: `_` — matches anything, binds nothing
- **Binding**: `x` — matches anything, binds to name `x`
- **Literal**: `42`, `"hello"`, `true` — matches by equality
- **Tuple**: `(a, b, _)` — destructures tuple elements
- **Struct**: `Foo { x, y: renamed }` — destructures struct fields
- **Enum**: `Some(x)`, `None`, `Shape::Circle(r)` — matches variant + destructures payload

### AST
- Add `Pattern` enum to AST: `Wildcard`, `Binding(name)`, `Literal(value)`, `Tuple(Vec<Pattern>)`, `Struct { name, fields }`, `Enum { variant, inner: Vec<Pattern> }`
- `Match { scrutinee: Expr, arms: Vec<MatchArm> }` where `MatchArm { pattern: Pattern, guard: Option<Expr>, body: Expr }`
- `IfLet { pattern: Pattern, scrutinee: Expr, then_block, else_block }`

### Match Expression Parsing
- `match expr { pattern => expr, pattern if guard => expr, ... }`
- Arms separated by commas
- Last arm can omit trailing comma
- Block bodies don't need comma: `pattern => { ... }`

### If-Let Parsing
- `if let pattern = expr { then } else { else }`

### Type Checking
- Validate patterns against scrutinee type
- Bindings in patterns create new variables with inferred types
- Guard expressions must be `bool`
- Match expression type: all arms must return the same type (or diverge)

### Exhaustiveness Checking
- Warn (or error) when match is not exhaustive
- For enums: all variants must be covered (or wildcard/binding present)
- For booleans: both true and false
- For integers/strings: wildcard required (can't enumerate)
- MVP: require a wildcard/catch-all arm if not all variants covered

### Lowering to IR
- Lower match to if-else chains:
  - Check discriminant for enum patterns
  - Compare values for literal patterns
  - Extract fields for struct/tuple/enum patterns
  - Bind variables for binding patterns
- No new IR instructions beyond what enums (Task 11) provide

## Acceptance Criteria
- `tests/control-flow/match_basic.nudl` compiles and runs
- `tests/control-flow/if_let.nudl` compiles and runs
- `tests/pattern-matching/literal_patterns.nudl` through `tests/pattern-matching/exhaustiveness.nudl`
- Match on integers with literal arms
- Match on enums with variant destructuring
- Match on structs with field extraction
- If-let with enum patterns
- Exhaustiveness warning for non-exhaustive matches
- Pattern bindings are properly scoped to their arm

## Technical Notes
- Pattern matching lowers to if-else chains in the IR — no special match instruction needed
- Enum matching: load tag → compare → branch; then extract payload fields
- Struct matching: extract fields by name → bind to pattern variables
- Tuple matching: extract elements by index → recurse on sub-patterns
- Literal matching: compare with `==`
- Exhaustiveness: implement a simple coverage checker (not full Maranget algorithm for MVP)
- Depends on: Task 11 (enums) for enum patterns, Task 06 (tuples) for tuple patterns
