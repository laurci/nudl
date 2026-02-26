# Task 10: Methods — `impl` Blocks and Method Calls

## Goal
Implement `impl` blocks for structs, `self`/`mut self` parameters, method call syntax (`obj.method(args)`), and static method syntax (`Type::method(args)`).

## Requirements

### Parsing `impl` Blocks
- `impl TypeName { fn method(self, ...) -> T { ... } }`
- `impl TypeName { fn method(mut self, ...) -> T { ... } }`
- `impl TypeName { fn static_method(...) -> T { ... } }` (no self param)
- Parse `self` and `mut self` as special first parameter
- Multiple `impl` blocks for the same type are allowed

### Method Calls
- `obj.method(args)` → desugars to `TypeName__method(obj, args)`
- Method resolution: look up methods on the type of `obj`
- Auto-referencing: not needed in MVP since structs are already reference types
- Chaining: `obj.method1().method2()` works naturally

### Static Methods
- `TypeName::method(args)` → calls `TypeName__method(args)` directly
- `Type::new(...)` pattern for constructors

### Name Mangling
- Methods are lowered to regular functions with mangled names: `TypeName__methodname`
- This avoids needing vtables or any special dispatch mechanism
- The mangled name is used in IR and codegen

### Type Checking
- Register methods in a per-type method table during declaration collection pass
- `self` type is the struct type; `mut self` allows mutation of fields
- Validate `self` is not used outside of method context
- Method calls: look up method on receiver type, validate args (excluding self)

## Acceptance Criteria
- `tests/functions/methods.nudl` compiles and runs
- Instance methods: `struct Foo { x: i32 } impl Foo { fn get_x(self) -> i32 { self.x } }`
- Mutation: `impl Foo { fn set_x(mut self, val: i32) { self.x = val; } }`
- Static methods: `impl Foo { fn new(x: i32) -> Foo { Foo { x } } }` called as `Foo::new(42)`
- Method chaining works
- Type errors: calling non-existent methods, wrong arg types

## Technical Notes
- Parse `impl` as a new top-level item type
- During type checking pass 1 (declaration collection), register methods in a `HashMap<TypeId, Vec<Method>>` or similar
- During type checking pass 2, resolve `.method()` calls by looking up the receiver's type
- Lowering: `obj.method(a, b)` → emit `TypeName__method(obj, a, b)` as a normal function call
- Name mangling with `__` separator (e.g., `Vec__push`, `String__len`)
- `self` param: the type checker treats it as an implicit parameter with the struct's type
- ARC: `self` follows normal struct ARC rules (retain on call, release when done)
