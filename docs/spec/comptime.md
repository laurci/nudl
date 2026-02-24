## 13. Compile-Time Evaluation (Comptime)

### 13.1 Overview

nudl provides compile-time evaluation through the `comptime` keyword. Code
marked as `comptime` is executed at compile time in a sandboxed virtual machine.
This enables metaprogramming: generating types, functions, and implementations
based on compile-time computations.

The comptime system is the primary metaprogramming mechanism in nudl. There are
no procedural macros. All metaprogramming is expressed as regular nudl code
executing in a constrained environment.

### 13.2 Comptime Blocks

```
comptime_block  = 'comptime' block_expr ;
```

A `comptime` block is evaluated at compile time. It may appear at the top level
of a module or inside a function body. The block's result must be either:

- A value serializable to a compile-time constant (primitives, strings, arrays
  of constants).
- A `quote` expression that produces AST fragments to be injected into the
  module.

```nudl
// Top-level comptime block generating a struct:
comptime {
    let names = ["x", "y", "z"];
    quote {
        struct Vec3 { ${for name in names { quote { ${name}: f64, } }} }
    }
}

// Comptime block inside a function producing a constant:
fn get_magic_number() -> i32 {
    comptime { 6 * 7 }
}
```

### 13.3 Comptime Functions

```
comptime_fn  = 'comptime' 'fn' identifier generics?
               '(' fn_params? ')' ( '->' type )? where_clause?
               block_expr ;
```

A comptime function can only be called in a comptime context. It has full access
to type reflection and code generation via `quote`.

```nudl
comptime fn make_point_struct(dims: u32) {
    let names = ["x", "y", "z", "w"];
    let fields = names[0..dims];
    quote {
        struct Point {
            ${for name in fields { quote { ${name}: f64, } }}
        }
    }
}

comptime {
    make_point_struct(3);
    // Generates: struct Point { x: f64, y: f64, z: f64 }
}
```

### 13.4 Types as Values

In comptime context, types are first-class values of the special kind `type`.
They can be stored in variables, passed to functions, placed in arrays, and
compared for equality.

```nudl
comptime fn numeric_types() -> type[] {
    [i8, i16, i32, i64, u8, u16, u32, u64, f32, f64]
}

comptime fn is_integer(T: type) -> bool {
    let integers = [i8, i16, i32, i64, u8, u16, u32, u64];
    for t in integers {
        if t == T { return true; }
    }
    false
}
```

Type values cannot escape comptime context. Using a `type` value in a runtime
expression is a compile-time error.

### 13.5 Comptime Parameters

Functions may accept comptime parameters: values that must be known at compile
time. The `comptime` keyword before a parameter name indicates this requirement.

```nudl
fn create_array<T>(comptime size: u32) -> [T; size] {
    [T::default(); size]
}

fn bit_set(comptime width: u32) -> [bool; width] {
    [false; width]
}
```

Comptime parameters can be used in type positions (such as the size of a
fixed-size array), enabling types to depend on compile-time-known values.

Comptime parameters may have default values, following the same rules as regular default parameters:

```nudl
fn create_buffer<T>(comptime size: u32 = 256) -> [T; size] {
    [T::default(); size]
}

let small = create_buffer<u8>();          // size = 256 (default)
let large = create_buffer<u8>(size: 4096); // size = 4096
```

### 13.6 Code Generation with Quote

The `quote` expression is the single primitive for code generation. It produces
an AST fragment that is injected into the module.

```
quote_expr  = 'quote' '{' quote_body '}' ;
```

Inside a `quote` block, `${}` splices in comptime values:

- **`${expr}`** — splice a comptime value (identifier, type, literal)
- **`${for pat in iter { quote { ... } }}`** — splice a repeated fragment
- **`${if cond { quote { ... } }}`** — conditionally splice a fragment
- **`$ident{expr}`** — splice an identifier constructed from a string expression.
  The expression must evaluate to a `string` at comptime, and the result is used
  as an identifier in the generated code.

The following splice forms are supported:

| Splice Form | Input Type | Output | Example |
|---|---|---|---|
| `${expr}` | `type` | Type in type position | `${T}` for a type variable |
| `${expr}` | `string` | String literal | `${name}` where name is a string |
| `${expr}` | integer/float/bool | Literal value | `${42}`, `${true}` |
| `$ident{expr}` | `string` | Identifier | `$ident{"get_" + field.name}` |
| `${quote { ... }}` | AST fragment | Nested code | `${quote { x + 1 }}` |
| `${for ...}` | iteration | Repeated fragment | See examples above |
| `${if ...}` | condition | Conditional fragment | See examples above |

`quote` can produce any top-level item: structs, enums, functions, impl blocks.
Generated code is injected into the compilation pipeline at the type-checking
stage and undergoes full type checking and lowering as if written by hand.

```nudl
comptime fn derive_printable(T: type) {
    let fields = type_fields(T);
    let name = type_name(T);

    quote {
        impl Printable for ${T} {
            fn to_string(self) -> string {
                let mut result = `${name} \{ `;
                ${for (i, field) in fields.enumerate() {
                    let fname = field.name;
                    quote {
                        ${if i > 0 { quote { result = `{result}, `; } }}
                        result = `{result}${fname}: {self.${fname}.to_string()}`;
                    }
                }}
                `{result} \}`
            }
        }
    }
}

struct Point { x: f64, y: f64 }
comptime { derive_printable(Point); }
// Generates:
// impl Printable for Point {
//     fn to_string(self) -> string { ... }
// }
```

### 13.7 AST Inspection

The `ast_of` function provides read-only access to the AST of existing items:

```nudl
comptime {
    let f = ast_of(my_function);
    f.name;                  // "my_function"
    f.params;                // [{name: "x", param_type: i32}, ...]
    f.return_type;           // the return type
    f.body;                  // body AST node
    f.body.statements;       // statement AST nodes
    f.body.statements[0].kind; // "let_binding", "expression", etc.
}
```

AST nodes are read-only comptime values. They can be inspected and used to
drive code generation via `quote`, but cannot be mutated.

### 13.8 Reflection API

The following built-in functions are available in comptime context for
inspecting types. These are regular functions (no special prefix):

| Function                   | Return Type  | Description                                      |
|----------------------------|-------------|--------------------------------------------------|
| `type_name(T)`             | `string`    | The name of type T                               |
| `type_fields(T)`           | `Field[]`   | Fields of a struct type                          |
| `type_methods(T)`          | `Method[]`  | Methods defined on a type                        |
| `type_variants(T)`         | `Variant[]` | Variants of an enum type                         |
| `type_implements(T, I)`    | `bool`      | Whether type T implements interface I            |
| `call_site()`              | `CallSite`  | Source location of the caller (file, line, col)  |
| `size_of(T)`               | `u64`       | Size of type T in bytes                          |
| `align_of(T)`              | `u64`       | Alignment of type T in bytes                     |
| `ast_of(item)`             | `AstNode`   | Read-only AST of an existing item                |
| `attributes(item)`         | `Attribute[]` | Attributes attached to an item                 |
| `field_attributes(T, name)` | `Attribute[]` | Attributes on a specific field                |
| `module_types()`           | `type[]`    | All struct/enum types defined in current module  |
| `module_functions()`       | `FnInfo[]`  | All functions defined in current module           |
| `module_interfaces()`      | `type[]`    | All interfaces defined in current module          |
| `imported_types()`         | `type[]`    | Types explicitly imported into current scope      |

The `Field` type provides `.name` (string), `.field_type` (type), and `.offset`
(u64). The `Method` type provides `.name` (string), `.params` (array of param
info), and `.return_type` (type). The `Variant` type provides `.name` (string)
and `.fields` (array of field info, empty for unit variants). The `Attribute`
type provides `.key` (string) and `.values` (comptime literal map). The
`FnInfo` type provides `.name` (string), `.params` (array of param info),
`.return_type` (type), and `.is_comptime` (bool).

#### Unrecognized Attributes

Unrecognized attributes are **silently accepted** by the compiler. They are stored in the type metadata and accessible via the `attributes()` and `field_attributes()` comptime functions. This enables user-defined attribute-driven code generation patterns:

```nudl
#[api_endpoint, method = "GET", path = "/users"]
fn list_users() -> User[] { ... }

// At comptime, reflect on attributes to generate routing:
comptime {
    for func in module_functions() {
        let attrs = attributes(func);
        if attrs.has("api_endpoint") {
            // generate route registration
        }
    }
}
```

Built-in attributes with compiler-level effects include: `#[derive(...)]`, `#[inline]`, `#[link_name]`, `#[extern_callable]`, `#[deprecated]`. All other attributes are user-defined.

#### Module-level introspection

Module-level introspection functions reflect over the **current module and its explicit imports**:

- `module_types()` — returns types defined in the current module, plus types brought in via `import` declarations (including glob imports).
- `module_functions()` — returns functions defined in the current module, plus explicitly imported functions.
- `module_interfaces()` — returns interfaces defined in the current module, plus explicitly imported interfaces.
- `imported_types()` — returns only the explicitly imported types (not locally defined ones).

Types from transitive dependencies that are not explicitly imported are not visible to reflection. This ensures reflection scope is predictable and matches what the programmer can reference by name.

Comptime code can iterate over all types, functions, and interfaces in the
current module. This enables patterns like "find all types with a given
attribute and generate code for them":

```nudl
comptime {
    for T in module_types() {
        if attributes(T).has("serializable") {
            let fields = type_fields(T);
            let name = type_name(T);
            quote {
                impl Serialize for ${T} {
                    fn to_json(self) -> string {
                        let mut parts: string[] = [];
                        ${for field in fields {
                            let fname = field.name;
                            quote {
                                parts.push(`"${fname}": {self.${fname}.to_json()}`);
                            }
                        }}
                        `\{ {parts.join(\", \")} \}`
                    }
                }
            }
        }
    }
}
```

The scope of module introspection is limited to the current module and its
explicit imports. Comptime blocks cannot see types from modules that have not
been imported. This avoids whole-program ordering dependencies — each module's
comptime evaluation depends only on its own definitions and its imports, which
are already resolved before comptime runs.

```nudl
comptime fn print_layout(T: type) {
    comptime_print(`Type: {type_name(T)}`);
    comptime_print(`  Size: {size_of(T)} bytes`);
    comptime_print(`  Align: {align_of(T)} bytes`);
    for field in type_fields(T) {
        comptime_print(`  Field '{field.name}': {type_name(field.field_type)} at offset {field.offset}`);
    }
}
```

### 13.9 Restrictions

Comptime evaluation operates under strict restrictions:

1. **No I/O.** File access, network operations, and printing are prohibited.
   `comptime_print` is the sole exception, emitting a compiler diagnostic note.

2. **No heap escape.** Reference-type values allocated during comptime cannot
   be returned to runtime code. Only primitive values, strings (serialized as
   constants), and fixed-size arrays of serializable values may cross the
   comptime-to-runtime boundary.

3. **Step limit.** The virtual machine enforces a configurable maximum number
   of instruction steps (default: 1,000,000). If exceeded, compilation fails
   with a diagnostic showing the call stack at the point of termination.

4. **Recursion depth limit.** Comptime code that emits code containing further
   comptime blocks is subject to a recursion depth limit (default: 16).

5. **No runtime function calls.** Comptime code may only call other comptime
   functions and the built-in reflection/generation intrinsics.

6. **No async.** `async` functions, `async` blocks, `.await`, and `await`
   expressions are forbidden in comptime context. Concurrency requires a
   runtime executor which is not available during compilation.

7. **Name conflicts.** If comptime-generated code produces an item (function,
   struct, enum, interface, or impl block) whose name collides with an existing
   item already defined in the same scope, the compiler reports an error. There
   is no implicit shadowing of existing items by generated code.

   ```
   error[E0510]: comptime-generated item `Point` conflicts with existing definition
     --> src/main.nudl:15:5
      |
   15 |     comptime { make_point_struct(3); }
      |     ^^^^^^^^ generates `struct Point` which conflicts with:
      |
   3  |     struct Point { x: f64, y: f64 }
      |     -------------------------------- previously defined here
   ```

**Interaction with build scripts:** Build scripts (`build.nudl`) execute in an
extended comptime VM with file-system I/O capabilities. If the build script
panics or returns `Err`, the build is aborted — no further compilation phases
run. The `add_define` function in a build script makes constants available to
comptime blocks in the main project.

**One-way boundary:** Build script definitions flow into comptime code (via
`build::add_define`), but comptime code cannot call `build::*` functions. The
`build` module is exclusively available in `build.nudl` and is not importable
from regular project code or comptime blocks. This ensures a clean one-way
dependency: build script → comptime, never reverse.

### 13.10 Interaction with Generics

Generics and comptime serve complementary purposes:

- **Generics** are the primary mechanism for type-parameterized code. Use
  generics when the same algorithm applies to multiple types with the same
  structure (containers, algorithms, interface-bounded functions).

- **Comptime** is the escape hatch for when generics are not expressive
  enough. Use comptime for deriving interface implementations, generating
  types from schemas, computing lookup tables, and other metaprogramming
  tasks.

```nudl
// Generics: same algorithm, different types
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

// Comptime: generating code based on type structure
comptime fn derive_clone(T: type) {
    let fields = type_fields(T);
    quote {
        impl Clone for ${T} {
            fn clone(self) -> ${T} {
                ${T} {
                    ${for field in fields {
                        let fname = field.name;
                        quote { ${fname}: self.${fname}.clone(), }
                    }}
                }
            }
        }
    }
}
```

### 13.11 Derive via Comptime

Interface implementations can be automatically generated using **comptime derive functions**. Unlike Rust's proc macros, nudl derives are ordinary comptime functions that use the reflection API to inspect types and emit implementations via `quote`.

#### Standard Library Derives

The standard library provides derive functions for core interfaces:

- `derive_clone(T)` — generates `Clone` by cloning each field
- `derive_eq(T)` — generates `Eq` by comparing each field
- `derive_ord(T)` — generates `Ord` by comparing fields in declaration order
- `derive_printable(T)` — generates `Printable` with `"TypeName { field: value, ... }"` format
- `derive_drop(T)` — generates `Drop` by dropping fields in declaration order (usually not needed — compiler inserts default drop)

Usage:

```nudl
#[derive(Clone, Eq, Printable)]
struct Point { x: f64, y: f64 }
```

The `#[derive(...)]` attribute is syntactic sugar. The compiler translates it to comptime calls:

```nudl
comptime {
    derive_clone(Point);
    derive_eq(Point);
    derive_printable(Point);
}
```

#### Custom Derives

Users can write custom derive functions using the same reflection API:

```nudl
comptime fn derive_serialize(comptime T: type) {
    let fields = type_fields(T);
    quote {
        impl Serialize for ${T} {
            fn serialize(self) -> string {
                let mut parts = string[];
                ${for field in fields {
                    quote { parts.push(`"${field.name}": {self.${field.name}.serialize()}`); }
                }}
                `{ ${type_name(T)} }\{ {parts.join(\", \")} \}`
            }
        }
    }
}

#[derive(Serialize)]
struct User { name: string, age: i32 }
```

When the compiler encounters `#[derive(X)]`, it looks for a function named `derive_x` (lowercased) in scope. If found, it is called at comptime with the annotated type. If not found, it is a compile error.

---

