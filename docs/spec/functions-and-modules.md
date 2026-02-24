## 11. Functions and Closures (Detailed)

### 11.1 Function Grammar

The complete grammar for function definitions:

```
fn_def       = visibility? 'comptime'? 'async'? 'fn' identifier generics?
               '(' fn_params? ')' ( '->' type )? where_clause?
               block_expr ;

fn_params    = fn_param ( ',' fn_param )* ','? ;

fn_param     = self_param | regular_param ;
self_param   = 'mut'? 'self' ;
regular_param = identifier '?'? ':' type ( '=' expression )? ;
```

- `async` and `comptime` are mutually exclusive — combining them is a compile
  error.
- An `async fn` with declared return type `T` has the effective type
  `(...) -> Future<T>`. The body executes lazily when the returned future is
  awaited. See Section 17 for full async semantics.
- The `self` parameter, if present, must be first.
- Regular parameters follow the ordering rule: required, then default, then
  optional.
- An optional parameter (`name?: Type`) has type `Option<Type>` in the body
  and defaults to `None` if omitted by the caller. Optional parameters can be
  explicitly passed `None` at the call site:

  ```nudl
  fn send(body?: string) { ... }
  send(body: None);       // equivalent to omitting body
  send(body: Some(data)); // explicit Some also works

  // Useful for computed optionality:
  send(body: if has_data { Some(data) } else { None });
  ```

- A default parameter (`name: Type = expr`) is evaluated at the call site if
  omitted. Default parameter expressions are evaluated **fresh at each call
  site** when the argument is omitted. Each call produces an independent
  evaluation:

  ```nudl
  fn connect(timeout: Duration = Duration::from_secs(30)) { ... }
  connect();  // evaluates Duration::from_secs(30) here
  connect();  // evaluates it again — independent call
  ```

  Any expression is valid as a default value, including function calls, field
  accesses, and complex expressions. The expression is type-checked against the
  parameter type at the function definition site.

### 11.2 Calling Convention

nudl's calling convention uses positional and named arguments:

1. **Self parameter** (`self` or `mut self`): invisible to callers. Method calls
   pass the receiver implicitly via the `.` syntax.

2. **First non-self parameter:** positional. The caller need not provide the
   parameter name. The caller *may* provide the name.

3. **All subsequent parameters:** named. The caller must provide the parameter
   name followed by `:`.

4. **Shorthand rule:** If a variable at the call site has the same name as a
   named parameter, the caller may write just the variable name instead of
   `name: name`.

```nudl
fn send(url: string, method: string = "GET", body?: string) -> Response {
    // ...
}

// Full syntax:
send("https://example.com", method: "POST", body: "data");

// First param is positional:
send("https://example.com", method: "POST");

// Shorthand for named params:
let method = "PUT";
let body = "payload";
send("https://example.com", method, body);
// Equivalent to: send("https://example.com", method: "PUT", body: "payload")
```

**Parameter ordering at the declaration site:**

```
required params  ->  default params  ->  optional params
```

Required parameters must precede default parameters, which must precede
optional parameters. Within each category, the order is as declared.

### 11.3 Closure Semantics

**Capture:** Closures capture variables from their enclosing scope **by ARC reference**. All captured variables — including value types — are boxed into a shared, ARC-managed capture environment. This means:

- Mutations to a captured variable inside the closure are visible in the enclosing scope, and vice versa.
- Value types (integers, bools, etc.) are boxed (heap-allocated) when captured. This has a small performance cost but provides consistent semantics.
- The closure and the enclosing scope share the same binding. The captured environment is freed when both the closure and the enclosing scope have released it.

```nudl
let mut count = 0;
let inc = || { count += 1; };
inc();
inc();
println(`{count}`);  // prints "2" — mutation is shared
```

This model is simpler than Rust's capture-by-move/reference distinction and aligns with nudl's ARC-everywhere philosophy. There is no `move` keyword for closures.

**Closure types:** A closure has type `(ParamTypes) -> ReturnType`, which is
the same type as a function pointer. This means closures and function
references are interchangeable.

**Closures are reference types:** They are heap-allocated and reference-counted.

### 11.4 Trailing Lambda Details

The trailing lambda syntax is an ergonomic shortcut for passing closures as
the last argument to a function. Full rules:

1. The last parameter of the function must have a function type.
2. The trailing block `{ ... }` is placed after the closing `)` of the
   argument list.
3. If the function has no other arguments, the `()` may be omitted.
4. **Single-parameter lambda:** The parameter is implicitly named `it`.
5. **Multi-parameter lambda:** Parameters are listed before `->`.
6. **Zero-parameter lambda:** The block contains only the body.

```nudl
// Definition:
fn with_retry(max_attempts: i32, action: () -> Result<i32, string>) -> i32 {
    // ...
}

// Call with trailing lambda:
let result = with_retry(3) {
    attempt_connection()
};

// Single param - implicit `it`:
let lengths = map(words) { it.len() };

// Multi param:
let products = zip_map(xs, ys: ys) { a, b -> a * b };

// Zero params, no other args:
fn run_test(test: () -> bool) -> bool { test() }
run_test { true }
```

Trailing lambdas use `{ }` syntax. Pipe closures (`|params| expr`) are for
inline closure expressions. The two forms are syntactically distinct and cannot
be mixed.

### 11.5 Method Resolution Ambiguity

When a type implements multiple interfaces that define methods with the same name, calling that method without qualification is a **compile error**:

```nudl
interface A { fn name(self) -> string; }
interface B { fn name(self) -> string; }

struct S {}
impl A for S { fn name(self) -> string { "A" } }
impl B for S { fn name(self) -> string { "B" } }

let s = S {};
s.name();           // ERROR: ambiguous — could be A::name or B::name
A::name(s);         // OK — qualified call
B::name(s);         // OK — qualified call
```

Inherent methods (defined directly in an `impl S` block, not for an interface) always take priority over interface methods and do not cause ambiguity.

---

## 12. Modules, Packages, and Visibility

### 12.1 Module Structure

Each `.nudl` source file defines a module. The module's path is determined by
its position in the file system relative to the project root.

**Project root resolution:** The project root is the directory containing
`nudl.toml` (see Section 12.4). If no `nudl.toml` exists, the directory
containing the entry source file is used.

```
project/
  nudl.toml               -> package manifest (marks project root)
  main.nudl               -> root module
  math/
    mod.nudl              -> math module
    vector.nudl           -> math::vector module
    matrix.nudl           -> math::matrix module
  util.nudl               -> util module
```

A directory becomes a module namespace when it contains a `mod.nudl` file. The
`mod.nudl` file may re-export items from submodules.

#### 12.1.1 Circular Imports

Circular imports between modules are allowed with restrictions. When two
modules reference each other's types, the compiler uses a **two-pass
resolution** strategy:

1. **First pass (forward declaration).** Types referenced across the cycle
   boundary are treated as opaque forward declarations. They can be used as
   ARC pointer types (e.g., in fields, function parameters, and return types)
   but their fields and methods are not yet accessible.

2. **Second pass (full resolution).** Once both modules have completed their
   first pass, the compiler resolves all forward declarations to their full
   definitions, making fields and methods available.

This works because reference types in nudl are always behind ARC pointers.
A forward-declared type has a known size (one pointer) even before its fields
are resolved.

```nudl
// file: parent.nudl
import child::Child;

pub struct Parent {
    name: string,
    child: Option<Child>,    // OK: Child is an ARC pointer
}

impl Parent {
    fn child_name(self) -> Option<string> {
        self.child.map { it.name.clone() }  // OK: resolved in second pass
    }
}

// file: child.nudl
import parent::Parent;

pub struct Child {
    name: string,
    parent: Option<weak Parent>,  // OK: Parent is an ARC pointer
}
```

**Restrictions:**

- Only type references are forward-declared. Functions, interfaces, and
  constants cannot participate in circular references.
- Accessing fields or methods of a forward-declared type before the cycle is
  resolved is a compile error (this can only happen in comptime blocks that
  execute during the first pass).
- Circular imports do not affect runtime behavior — they are purely a
  compile-time resolution concern.

### 12.2 Imports

```
import_decl  = 'import' import_path ';' ;

import_path  = path
             | path '::' '{' import_item % ',' '}'
             | path '::' '*' ;

import_item  = identifier ( 'as' identifier )? ;
```

```nudl
import std::collections::Map;
import math::{Vector, Matrix};
import math::vector::Vector3 as Vec3;
import std::io::*;                         // glob import (discouraged)
```

**Dependency imports:** When a package has dependencies declared in `nudl.toml`
(see Section 12.4), the dependency key name becomes the import root:

```nudl
// Given nudl.toml: [dependencies] http = "github.com/user/nudl-http"
import http::Client;
import http::{Request, Response};
```

**Glob imports** (`*`) import all public items from a module. They are
discouraged because they make it unclear where names originate.

**Aliasing** (`as`) provides an alternative name for the imported item. The
original name is not available; only the alias is bound.

### 12.3 Visibility Rules

- Items are **private** by default: visible only within the module where they
  are defined.
- `pub` items are visible to any module that imports them.
- Struct fields have independent visibility. A struct may be `pub` while some
  of its fields are private. Private fields cannot be accessed or set from
  outside the defining module.
- All variants of a `pub` enum are automatically public.
- `pub` on a method in an `impl` block makes that method part of the type's
  public API.
- Local items — functions, structs, enums, or other declarations inside a
  function body — cannot have `pub` visibility. The `pub` modifier on a local
  item is a compile error. Local items are always scoped to their enclosing
  block.

```nudl
pub struct Config {
    pub host: string,
    pub port: u16,
    secret_key: string,        // private
}

// Outside the module:
let c = Config { host: "localhost", port: 8080, secret_key: "..." };
// ERROR: field `secret_key` is private
```

### 12.4 Package Manifest (nudl.toml)

#### 12.4.1 Overview

A `nudl.toml` file at the project root marks the project as a package and
configures its metadata, dependencies, and build options. Without a `nudl.toml`,
a project is treated as a single-file program with default settings.

#### 12.4.2 Format

```toml
[package]
name = "my-project"
version = "0.1.0"
description = "A short description"
authors = ["Author <email>"]
license = "MIT"
nudl-version = "0.1"

[dependencies]
http = "github.com/user/nudl-http"
json = "github.com/user/nudl-json@v1.2.0"
utils = "github.com/user/nudl-utils#abc1234"
local-lib = { path = "../my-lib" }

[build]
entry = "main.nudl"
target = "aarch64-apple-darwin"
output = "my-project"

[build.flags]
opt-level = 2
comptime-step-limit = 2000000
deny-warnings = false
```

#### 12.4.3 Package Section

| Field | Required | Description |
|---|---|---|
| `name` | Yes | Package name. Must be a valid nudl identifier. |
| `version` | Yes | Semantic version string (e.g., `"0.1.0"`). |
| `description` | No | Short description of the package. |
| `authors` | No | Array of author strings. |
| `license` | No | SPDX license identifier. |
| `nudl-version` | No | Minimum nudl language version required. |

#### 12.4.4 Dependencies

Dependencies use a Go-style source distribution model: packages are always
fetched as source code from Git repositories.

**Specifier formats:**

| Format | Example | Description |
|---|---|---|
| URL (default branch) | `"github.com/user/repo"` | Latest commit on default branch |
| URL with tag | `"github.com/user/repo@v1.2.0"` | Specific Git tag |
| URL with commit | `"github.com/user/repo#abc1234"` | Specific commit hash |
| Local path | `{ path = "../my-lib" }` | Local filesystem path |

The dependency key name becomes the import root: a dependency named `http`
allows `import http::Client;`.

**Single-version policy:** A given package may appear at most once in the
resolved dependency graph. Diamond dependencies that require different versions
of the same package are compile errors. This simplifies the module system and
avoids the "dependency hell" of multiple coexisting versions.

**Fetch location:** Dependencies are fetched into `.nudl/deps/` within the
project root.

**Lock file:** `.nudl/deps.lock` records the exact resolved commit hash and
content hash of each dependency, ensuring reproducible builds. The lock file
should be committed to version control.

#### 12.4.5 Link Section

The `[build.link]` table configures native library linking for FFI:

```toml
[build.link]
search-paths = ["/opt/homebrew/lib", "/usr/local/lib"]
libs = ["sqlite3", "z"]
framework-paths = ["/Library/Frameworks"]    # macOS only
frameworks = ["Security", "CoreFoundation"]  # macOS only
```

| Field | Type | Description |
|---|---|---|
| `search-paths` | `string[]` | Additional library search directories (`-L`) |
| `libs` | `string[]` | Additional libraries to link (`-l`). No `lib` prefix. |
| `framework-paths` | `string[]` | Additional framework search directories (`-F`, macOS only) |
| `frameworks` | `string[]` | Additional frameworks to link (`-framework`, macOS only) |

Libraries specified here are linked in addition to any libraries specified
inline in extern blocks. See Section 19.4 for the full linking model.

#### 12.4.6 Build Section

| Field | Default | Description |
|---|---|---|
| `entry` | `"main.nudl"` | Entry point source file |
| `target` | Platform default | Compilation target triple |
| `output` | Package name | Output binary name |

#### 12.4.7 Build Flags

| Flag | Default | Description |
|---|---|---|
| `opt-level` | `0` | Optimization level (0 = none, 1 = basic, 2 = full) |
| `comptime-step-limit` | `1000000` | Maximum VM steps for comptime evaluation |
| `deny-warnings` | `false` | Treat warnings as errors |

#### 12.4.8 Interaction with Module System

The project root (directory containing `nudl.toml`) establishes the root of
the module tree. Dependency packages form separate module trees, accessible via
their dependency key name as the root path segment.

```
my-project/
  nudl.toml
  main.nudl                -> root module
  lib/
    mod.nudl               -> lib module
  .nudl/
    deps/
      nudl-http/           -> http::* modules
      nudl-json/           -> json::* modules
    deps.lock              -> lock file
    generated/             -> build script output (see §12.5)
```

### 12.5 Build Scripts

#### 12.5.1 Overview

A `build.nudl` file at the project root is a build script that runs before
main compilation. It executes in an extended comptime VM with limited I/O
capabilities, enabling build-time code generation, environment detection, and
configuration.

#### 12.5.2 Execution Model

The build script executes as a distinct phase in the compilation pipeline,
inspired by Cargo's build script model:

1. The compiler reads `nudl.toml` and resolves dependencies.
2. The compiler compiles `build.nudl` to SSA bytecode and executes it in the
   comptime VM with extended capabilities (see §12.5.3).
3. The build script may produce: compiler flags, generated source files (written
   to `.nudl/generated/`), and build-time constants.
4. The main project compiles with generated files included in the module tree
   and flags applied.

#### 12.5.3 Build Script API

The `build` module is automatically available in build scripts (it is not
importable from normal project code). It provides the following functions:

| Function | Return | Description |
|---|---|---|
| `build::set_flag(key: string, value: string)` | `()` | Set a compiler flag |
| `build::add_define(name: string, value: string)` | `()` | Define a comptime constant for the main project |
| `build::generate_file(path: string, content: string)` | `()` | Write a file to `.nudl/generated/` |
| `build::read_file(path: string)` | `Result<string, Error>` | Read a file relative to the project root |
| `build::env(name: string)` | `Option<string>` | Read an environment variable |
| `build::package_name()` | `string` | Package name from nudl.toml |
| `build::package_version()` | `string` | Package version from nudl.toml |
| `build::target()` | `string` | Current compilation target triple |

**Key distinction:** Regular comptime code is fully sandboxed (no I/O). Build
scripts have limited I/O: they can read project files and environment variables,
and write to `.nudl/generated/`. They cannot access the network, write to
arbitrary paths, or execute external processes.

#### 12.5.4 Structure

A build script is a regular nudl source file with a `main` function:

```nudl
import build;

fn main() {
    let version = build::read_file("VERSION").unwrap_or("0.0.0");
    build::add_define("APP_VERSION", version);

    build::generate_file("version.nudl", `
        pub fn version() -> string \{ "{version}" \}
    `);
}
```

Generated files are accessible as modules under the `generated` namespace:

```nudl
// In main project code:
import generated::version;

fn main() {
    println(`Version: {version::version()}`);
}
```

#### 12.5.5 Restrictions

- **Separate step limit.** Build scripts have a higher default step limit
  (10,000,000) than regular comptime blocks, configurable via
  `comptime-step-limit` in `[build.flags]`.
- **Cannot import main project modules.** The build script runs before the
  main project is compiled. It can import project dependencies declared in
  `nudl.toml`.
- **No network I/O.** Build scripts cannot make network requests.
- **Writes restricted to `.nudl/generated/`.** The `build::generate_file`
  function only writes within this directory. Attempts to write elsewhere
  (e.g., using `../` path traversal) are compile errors.

---

