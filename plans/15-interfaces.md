# Task 15: Interfaces (Declaration, Implementation, Resolution, Bounds)

## Goal
Implement `interface` declarations, `impl Interface for Type` blocks, method resolution, generic bounds (`T: Interface`), and `where` clauses.

## Requirements

### Parsing — Interface Declarations
- `interface Name { fn method(self) -> RetType; }` — method signatures (no body = required)
- `interface Name { fn method(self) -> RetType { default_body } }` — default method implementations
- Generic interfaces: `interface Iterator<T> { fn next(mut self) -> Option<T>; }`
- Multiple methods per interface

### Parsing — Implementation
- `impl Interface for Type { fn method(self) -> RetType { body } }`
- Generic implementations: `impl<T> Display for Wrapper<T> { ... }`
- All required methods must be implemented; default methods can be overridden

### Parsing — Bounds and Where Clauses
- Inline bounds: `fn sort<T: Ord>(arr: T[]) -> T[]`
- Multiple bounds: `fn print_sorted<T: Ord + Display>(arr: T[])`
- Where clauses: `fn foo<T, U>(a: T, b: U) -> bool where T: Eq, U: Eq { ... }`
- Where clauses on impl blocks: `impl<T> Wrapper<T> where T: Display { ... }`

### AST Changes (`nudl-ast/src/ast.rs`)
- Add `Item::InterfaceDef { name, type_params, methods, is_pub, span }`
- Add `InterfaceMethod { name, params, return_type, body: Option<Block>, span }`
- Extend `ImplBlock` with `interface_name: Option<Symbol>`, `interface_type_args: Vec<TypeExpr>`
- Add `WhereClause { type_name: Symbol, bounds: Vec<TypeExpr> }`
- Add `where_clauses: Vec<WhereClause>` to `FnDef` and `ImplBlock`
- Extend `GenericParam` bounds field to support `+` syntax: `T: A + B`

### Parser Changes (`nudl-ast/src/parser.rs`)
- Parse `interface Name<T> { method_signatures_and_defaults }`
- Parse `impl Interface for Type { methods }`
- Parse `where T: A + B, U: C` after fn signature or impl header
- Parse bounds `<T: Ord>` on `GenericParam` (colon + type list with `+`)

### Type System Changes (`nudl-core/src/types.rs`)
- Add `TypeKind::Interface { name, type_params, methods }` — checker-only, not a runtime type

### Type Checker Changes (`nudl-bc/src/checker.rs`)
- `InterfaceInfo { name, type_params, methods, default_impls }`
- `interfaces: HashMap<Symbol, InterfaceInfo>`
- `interface_impls: HashMap<(TypeId, Symbol), ImplInfo>` — maps (type, interface) to implementation
- Method resolution order: inherent methods (from `impl Type`) → interface methods (from `impl Interface for Type`) → ambiguity error
- Qualified dispatch: `Interface::method(obj)` for disambiguation
- `Self` type substitution in impl blocks
- Bounds enforcement: at monomorphization time, verify that concrete type implements all required interfaces
- Bounds error messages: "type `i32` does not implement interface `Display`"

### Method Name Mangling
- Interface methods: `TypeName__InterfaceName__method` (three-part)
- Inherent methods: `TypeName__method` (two-part, unchanged from Task 10)
- This ensures no collision between inherent and interface methods of the same name

## Acceptance Criteria
- `tests/interfaces/declaration.nudl` — declare interfaces with required and default methods
- `tests/interfaces/implementation.nudl` — implement interfaces for concrete types
- `tests/interfaces/inherent_methods.nudl` — inherent methods take priority over interface methods
- `tests/interfaces/generic_interfaces.nudl` — generic interfaces like `Iterator<T>`
- `tests/interfaces/method_resolution.nudl` — correct method dispatch, qualified disambiguation
- `tests/generics/bounds.nudl` — `<T: Interface>` bounds are enforced
- `tests/generics/where_clauses.nudl` — `where T: A + B` syntax works
- Missing interface impl is a compile error
- Missing required method in impl is a compile error
- `cargo test --workspace` passes

## Technical Notes
- Interfaces in nudl are NOT traits — no associated types; generics fill that role (e.g., `Iterator<T>`, `Index<Idx, Output>`)
- Method resolution is static dispatch (monomorphized) — `dyn` dispatch comes in Task 16
- Default methods are optional to override; if not overridden, the default body is used (monomorphized for the implementing type)
- `Self` in interface method signatures refers to the implementing type
- Bounds are only checked at monomorphization boundaries — inside a generic function, method calls on `T: Interface` are resolved to the interface's method signatures
- The interface itself is not a runtime type — it's purely a compile-time constraint
- Depends on: Task 13 (generic function infrastructure), Task 14 (generic structs/enums for generic impls)
