# Task 14: Generic Structs, Enums & Turbofish

## Goal
Add type parameters to struct and enum definitions, implement generic type application (instantiation), and add turbofish syntax (`::< >`) for explicit type arguments at call sites.

## Requirements

### Parsing
- Generic structs: `struct Pair<A, B> { first: A, second: B }`
- Generic enums: `enum Option<T> { Some(T), None }`
- Generic impl blocks: `impl<A, B> Pair<A, B> { fn swap(self) -> Pair<B, A> { ... } }`
- Turbofish on function calls: `identity::<i32>(42)`
- Turbofish on static calls: `Pair::<i32, string>::new(1, "hi")`
- Type arguments on struct literals: `Pair::<i32, f64> { first: 1, second: 2.0 }`

### AST Changes (`nudl-ast/src/ast.rs`)
- Add `type_params: Vec<GenericParam>` to `StructDef`, `EnumDef`
- Add `type_args: Vec<TypeExpr>` to `StructLiteral`, `Call`, `StaticCall`, `MethodCall`, `ImplBlock`

### Parser Changes (`nudl-ast/src/parser.rs`)
- Parse `<T>` after struct/enum name in definitions
- Parse turbofish `::< >` in expression position: after `::` followed by `<`, parse as type args (not comparison)
- Parse generic impl blocks: `impl<A, B> Pair<A, B> { ... }`

### Type Checker Changes (`nudl-bc/src/checker.rs`)
- Store generic type definitions: `generic_structs: HashMap<Symbol, GenericStructDef>`, `generic_enums: HashMap<Symbol, GenericEnumDef>`
- Monomorphize structs/enums lazily on first use with concrete type args
- Turbofish bypasses type inference — uses explicit type args directly
- Methods on generic types: `impl<T> Wrapper<T>` — monomorphize the impl block per concrete type instantiation
- Monomorphization cache for types: `(type_name, Vec<TypeId>) → MonomorphizedTypeId`

### Name Mangling
- Types: `Pair__i32_string`
- Methods on generic types: `Pair__i32_string__swap`
- Enum variants: `Option__i32::Some`, `Option__i32::None`

## Acceptance Criteria
- `tests/generics/generic_structs.nudl` — generic struct definition, instantiation, field access
- `tests/generics/generic_enums.nudl` — generic enum definition, variant construction
- `tests/generics/turbofish.nudl` — explicit type args with `::< >`
- `tests/generics/monomorphization.nudl` — multiple instantiations produce separate types
- `Pair<i32, string>` and `Pair<f64, bool>` are distinct types
- Methods on generic types work: `impl<T> Wrapper<T> { fn get(self) -> T { self.value } }`
- ARC works correctly for generic reference types
- `cargo test --workspace` passes

## Technical Notes
- Generic structs/enums are templates, not real types — only monomorphized instances exist at runtime
- Turbofish disambiguation: `::` before `<` is the key signal. In expression `foo::<i32>(x)`, the `::` distinguishes from `foo < i32`
- For generic enums, each monomorphized variant may have different payload sizes
- Generic impl blocks: when `impl<T> Wrapper<T>` is monomorphized for `Wrapper<i32>`, all methods in the block are monomorphized with `T = i32`
- Depends on: Task 11 (enums for generic enums), Task 13 (generic function infrastructure, monomorphization cache)
