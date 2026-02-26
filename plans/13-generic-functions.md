# Task 13: Generic Functions (Monomorphization)

## Goal
Parse type parameters on `fn`, perform type inference at call sites, and implement the monomorphization pipeline that generates concrete instantiations of generic functions.

## Requirements

### Parsing
- Type parameters on functions: `fn identity<T>(x: T) -> T { x }`
- Multiple type params: `fn swap<A, B>(a: A, b: B) -> (B, A) { (b, a) }`
- Generic type application in type position: `Type<Args>` (e.g., return types, parameter types)
- Reserve `where` keyword for future use (Task 15)

### AST Changes (`nudl-ast/src/ast.rs`)
- Add `GenericParam { name: Symbol, bounds: Vec<TypeExpr>, span: Span }`
- Add `type_params: Vec<GenericParam>` to `FnDef`
- Add `TypeExpr::Generic { name: Symbol, type_args: Vec<TypeExpr> }` for generic type references

### Parser Changes (`nudl-ast/src/parser.rs`)
- Parse `<T, U>` after fn name: `fn name<T, U>(...)` — distinguish from comparison operators by context (after `fn name`)
- Parse `Type<Args>` in type position (parameter types, return types, let type annotations)

### Lexer Changes (`nudl-ast/src/token.rs`)
- Reserve `where` as a keyword

### Type System Changes (`nudl-core/src/types.rs`)
- Add `TypeKind::TypeVar { name: Symbol }` — placeholder for unresolved type parameters

### Type Checker Changes (`nudl-bc/src/checker.rs`)
- Store generic function definitions separately: `generic_fns: HashMap<Symbol, GenericFnDef>`
- On call to a generic function:
  1. Infer type arguments by unifying call argument types with parameter types
  2. Substitute type vars → concrete types
  3. Check monomorphization cache: `(fn_name, Vec<TypeId>) → mangled_name`
  4. If not cached, emit a `MonomorphRequest { fn_name, type_args, mangled_name }`
- The call site references the mangled name, not the generic name

### Lowering Changes (`nudl-bc/src/lower.rs`)
- Process `MonomorphRequest` list: re-lower the generic fn body with a type substitution map `TypeVar → ConcreteType`
- Each monomorphized instance becomes a separate `Function` in the IR `Program`

### Name Mangling
- Pattern: `fn_name__type1_type2` (e.g., `identity__i32`, `max__f64`, `swap__i32_string`)
- Double underscore separates fn name from type args
- Single underscore separates multiple type args

## Acceptance Criteria
- `tests/generics/generic_functions.nudl` compiles and runs
- Identity function: `fn identity<T>(x: T) -> T { x }` works with i32, f64, bool, string
- Max function: `fn max<T>(a: T, b: T) -> T` works with numeric types
- Multiple type params: `fn swap<A, B>(a: A, b: B) -> (B, A)` works
- Type inference at call sites: `identity(42)` infers `T = i32`
- Each instantiation produces a separate function in the IR/LLVM output
- `cargo test --workspace` passes

## Technical Notes
- Generic functions are NOT compiled directly — they serve as templates
- Only monomorphized instances appear in the final IR and LLVM output
- Type inference uses unification: walk parameter types and argument types in parallel, collecting `TypeVar → Type` mappings
- If a type variable appears multiple times, all inferred types must agree (e.g., `fn eq<T>(a: T, b: T)` requires both args same type)
- If inference fails or is ambiguous, the user must use turbofish (Task 14) or explicit type annotations
- No bounds checking in this task — any type can be used; bounds come in Task 15
- Depends on: nothing (independent of previous phases, though benefits from existing fn infrastructure)
