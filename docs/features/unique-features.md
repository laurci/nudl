## 4. What nudl Does Differently

### 4.1 ARC Without a Borrow Checker

nudl divides all types into two categories:

**Value types** live on the stack and are copied on assignment:
- Primitives: `i8`..`i64`, `u8`..`u64`, `f32`, `f64`, `bool`, `char`
- Tuples of value types, fixed-size arrays `[T; N]`, unit `()`

**Reference types** live on the heap and are reference-counted:
- Structs, enums, `string`, dynamic arrays `T[]`, `Map<K, V>`, closures, `dyn Interface`

When you assign a reference type, the reference count increments -- both bindings alias the same
allocation. Mutation through any binding affects the shared data:

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a;       // refcount++, shared alias
b.x = 99.0;
println(f"{a.x}");   // prints 99.0

let mut c = a.clone();  // deep copy, independent allocation
c.x = 0.0;
println(f"{a.x}");   // still 99.0
```

**Aliased mutation — the most important thing to understand:**

Because reference types are shared (not copied) on assignment, mutations through one binding are
visible through all other bindings to the same object:

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a;       // same object, refcount = 2
b.x = 99.0;
println(f"{a.x}");   // prints 99.0

// Use .clone() for an independent copy:
let mut c = a.clone();
c.x = 0.0;
println(f"{a.x}");   // still 99.0 — c is independent
```

This applies to function arguments too — if a function receives a struct and mutates it, the caller
sees the change. Clone before mutating if you need isolation. This model is identical to Swift
classes, Java objects, and Python objects.

**Weak references** break reference cycles:

```nudl
struct Node {
    value: i32,
    parent: Option<weak Node>,
    children: Node[],
}

let weak parent_ref = root;
match parent_ref.upgrade() {
    Some(p) => println(f"Parent: {p.value}"),
    None => println("Parent was deallocated"),
}
```

The compiler performs cycle detection at compile time and emits warnings when potential cycles lack
weak references.

**v1 note:** In version 1, nudl always generates retain/release for every reference-type
operation. Move-on-last-use optimization (eliding retain/release when the source binding is not
used after the assignment) is planned as a future optimization. If the ARC strong or weak count
reaches `u32::MAX`, the program aborts.

### 4.2 Named Arguments with Positional First-Param Exemption

The first non-self parameter is positional. All subsequent parameters must be named at the call
site. Parameter ordering: **required** then **defaults** then **optional**:

```nudl
fn send_request(
    url: string,              // required, first = positional
    method: string = "GET",   // default value
    body?: string,            // optional, desugars to Option<string>
) -> Result<Response, Error> { /* ... */ }

send_request("https://api.example.com", method: "POST", body: "{\"key\": 42}");

// Shorthand: variable name matches param name, skip the label
let method = "POST";
send_request("https://api.example.com", method);
```

### 4.3 Trailing Lambdas with Implicit `it`

When the last parameter of a function is a function type, the caller can pass it as a trailing
block. Single-parameter lambdas get an implicit `it` binding:

```nudl
let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

// Single-param: implicit `it`
let evens = filter(numbers) { it % 2 == 0 };

// Multi-param: name them explicitly
let sum = fold(numbers, initial: 0) { acc, item -> acc + item };
```

Trailing lambdas use `{ }` exclusively. Freestanding closures use pipe syntax:

```nudl
let double = |x: i32| x * 2;           // freestanding closure
let doubled = map(numbers) { it * 2 };  // trailing lambda
```

### 4.4 Comptime and Generics

**Generics** are the primary mechanism, monomorphized at compile time:

```nudl
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}
```

**Comptime** is the escape hatch for code generation and reflection. Types are first-class values:

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

comptime { make_point_struct(3); }
// Generates: struct Point { x: f64, y: f64, z: f64 }
```

The **comptime parameter modifier** lets functions accept compile-time-known values:

```nudl
fn create_array<T>(comptime size: u32) -> [T; size] {
    [T::default(); size]
}
```

**Module introspection** lets comptime iterate over all types in the current module:

```nudl
comptime {
    for T in module_types() {
        if attributes(T).has("printable") {
            // generate Printable impl for T
        }
    }
}
```

Scope is limited to the current module and its imports — no whole-program ordering issues.

**Derive via Comptime** — Interface implementations can be auto-generated using
`#[derive(Clone, Eq, Printable)]`. Unlike Rust's proc macros, nudl derives are ordinary comptime
functions that use the reflection API. The standard library ships derives for `Clone`, `Eq`, `Ord`,
`Printable`, and `Drop`. Users can write custom derives using the same comptime reflection API.

**Comptime restrictions:** no I/O, no heap allocations that escape to runtime, step limit to prevent
infinite loops, executed in a sandboxed VM via SSA bytecode.

### 4.5 Defer

`defer` schedules a block to execute on scope exit (LIFO order), regardless of how the scope exits:

```nudl
fn process_file(path: string) -> Result<string, Error> {
    let file = open(path)?;
    defer { file.close(); }

    let lock = file.lock()?;
    defer { lock.release(); }

    let content = file.read_all()?;
    // On exit: lock.release() first, then file.close()
    Ok(content)
}
```

### 4.6 Spread Operator

The spread operator `...` works in both array and struct contexts:

```nudl
let combined = [...prefix, 4, 5, 6, ...suffix];

let production = Config {
    ...defaults,
    host: "prod.example.com",
    port: 443,
    tls: true,
};
```

### 4.7 Async/Await and Actors

nudl provides single-threaded cooperative concurrency with structured concurrency guarantees. Async
functions are compiled to state machines, and child tasks cannot outlive their parent scope.

**Async functions and await:**

```nudl
async fn fetch_user(id: i32) -> User {
    let response = http_get(f"/users/{id}").await;
    parse_json(response.body()).await
}

async fn main() {
    let user = fetch_user(42).await;
    println(f"Hello, {user.name}!");
}
```

Both postfix `.await` (for chaining) and prefix `await` (for capturing entire expressions) are
supported:

```nudl
// Postfix: high precedence, great for chaining
let body = fetch(url).await.json().await;

// Prefix: low precedence, captures everything
await long_computation()
```

**Structured concurrency with Task.spawn and Task.group:**

```nudl
// Spawn individual tasks (lifetime bound to scope)
let handle = Task.spawn(async { expensive_computation().await });
let result = handle.await;

// Fan-out with task groups
let pages = Task.group { group ->
    for url in urls {
        group.spawn(async { fetch(url).await });
    }
};
// pages: string[] -- all results collected, ordered by spawn
```

**Actors** provide concurrent objects with isolated mutable state:

```nudl
actor ChatRoom {
    messages: string[],
    users: string[],

    fn new() -> ChatRoom {
        ChatRoom { messages: [], users: [] }
    }

    fn join(mut self, name: string) {
        self.users.push(name);
        self.messages.push(f"{name} joined");
    }

    fn send(mut self, user: string, text: string) {
        self.messages.push(f"{user}: {text}");
    }

    fn history(self) -> string[] {
        self.messages.clone()
    }
}

async fn main() {
    let room = ChatRoom::new();
    room.join("Alice").await;     // external calls are implicitly async
    room.send("Alice", text: "Hello!").await;
    let msgs = room.history().await;
}
```

**Executor:** The async executor is built-in and implicit. `async fn main()` is a valid entry
point — no manual executor setup is required.

**Actor isolation:** Actor methods must not return references to internal state (compile error).
They can return clones or extracted value types. Actors may hold references to other actors.

**Actor method dispatch:** Actor methods called on `self` execute **synchronously** (no message
queue). External calls to an actor are async with **FIFO-per-sender** ordering.

**Cancellation:** When a task is cancelled, it is dropped at the next `.await` suspension point.
The stack unwinds normally — `defer` blocks and `Drop` implementations execute during cleanup.
`Task.is_cancelled()` and `Task.check_cancelled()` allow checking between await points.

### 4.8 Pipe Operator

The pipe operator `|>` enables F#/Elixir-style data pipelines, passing the left-hand value as the
first positional argument to the right-hand function:

```nudl
// Without pipe -- deeply nested, reads inside-out:
let result = fold(map(filter(data) { it > 0 }) { it * 2 }, initial: 0) { acc, x -> acc + x };

// With pipe -- linear, reads top-to-bottom:
let result = data
    |> filter { it > 0 }
    |> map { it * 2 }
    |> fold(initial: 0) { acc, x -> acc + x };
```

Desugaring rules:
- `x |> f` becomes `f(x)`
- `x |> f(y, z)` becomes `f(x, y, z)`
- `x |> obj.method` becomes `obj.method(x)`
- `x |> obj.method(y)` becomes `obj.method(x, y)`

The pipe operator interacts naturally with named arguments and trailing lambdas:

```nudl
let report = raw_data
    |> validate { it.is_valid() }
    |> transform(format: "json")
    |> send(endpoint: "/api/reports", method: "POST");
```

### 4.9 Source-Only Dependencies

nudl uses a Go-style dependency model: packages are always distributed as source code from Git
repositories. There is no package registry — the dependency specifier is the repository URL.

```toml
# nudl.toml
[package]
name = "my-app"
version = "0.1.0"

[dependencies]
http = "github.com/user/nudl-http"
json = "github.com/user/nudl-json@v1.2.0"
utils = { path = "../shared-utils" }
```

```nudl
import http::Client;
import json::parse;

fn main() {
    let client = Client::new();
    // ...
}
```

**Why source-only?** Source distribution means the nudl compiler can apply whole-program
optimizations, monomorphize generics across dependency boundaries, and run comptime code from
dependencies. It also avoids ABI compatibility concerns.

**Single-version policy:** Each package may appear at most once in the resolved dependency graph.
Diamond conflicts are compile errors, keeping the module system simple.

**Project layout with dependencies:**

```
my-app/
  nudl.toml
  main.nudl
  .nudl/
    deps/
      nudl-http/        -> import http::*
      nudl-json/        -> import json::*
    deps.lock           -> reproducible builds
```

### 4.10 Build Scripts as Extended Comptime

`build.nudl` is a build script that runs before main compilation in an extended comptime VM. It
can read project files, inspect environment variables, and generate source code — bridging the gap
between nudl's sandboxed comptime and build-time configuration needs.

```nudl
// build.nudl
import build;

fn main() {
    // Read version from a file
    let version = build::read_file("VERSION").unwrap_or("0.0.0");
    build::add_define("APP_VERSION", version);

    // Detect target and set flags
    if build::target() == "aarch64-apple-darwin" {
        build::set_flag("macos-framework", "true");
    }

    // Generate code from a schema
    let schema = build::read_file("schema.json").unwrap();
    build::generate_file("models.nudl", generate_models(schema));
}

fn generate_models(schema: string) -> string {
    // Parse schema and generate struct definitions...
    f"pub struct User {{ name: string, age: u32 }}"
}
```

**Key distinction from regular comptime:** Regular comptime is fully sandboxed (no I/O). Build
scripts have limited I/O (read project files, environment variables, write to `.nudl/generated/`).
No network access, no arbitrary file writes, no exec.

---
