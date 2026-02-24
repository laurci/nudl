## 11. Implementation Architecture

### Phase 1: Core Infrastructure (nudl-core, nudl-ast)

**nudl-core** provides shared types: source locations, diagnostics, error types, type
representations. **nudl-ast** handles lexing and parsing: source text to token stream to AST.

### Phase 2: Type System and IR (nudl-bc)

Type checking, interface resolution, monomorphization, AST-to-SSA bytecode lowering, and ARC
retain/release insertion.

### Phase 3: Compile-Time Execution (nudl-vm)

SSA bytecode interpreter for comptime evaluation. Generated code feeds back to Phase 2. Sandboxed:
no I/O, step limits.

### Phase 4: Native Compilation (nudl-backend-arm64, nudl-packer-macho, nudl-packer-elf)

ARM64 instruction selection and register allocation. Mach-O packaging for macOS, ELF packaging for
Linux.

### Phase 5: Tooling (nudl-cli, nudl-lsp)

CLI frontend (`nudl build`, `nudl run`, `nudl check`, `nudl fmt`) and Language Server Protocol (go-to-definition,
find-references, hover, completions).

### Pipeline Overview

```
Source --> [Lexer] --> Tokens --> [Parser] --> AST
  --> [Type Checker] --> Typed AST --> [SSA Lowering] --> SSA Bytecode
  --> [VM (comptime)] --> Generated Code --> back to Type Checker
  --> [ARM64 Backend] --> Machine Code --> [Packer] --> Executable
```

---

## 12. Example Programs

### 12.1 Hello World

```nudl
fn main() {
    println("Hello, world!");
}
```

### 12.2 Fibonacci

```nudl
fn fib(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        n => fib(n - 1) + fib(n - 2),
    }
}

fn fib_iter(n: u64) -> u64 {
    if n == 0 { return 0; }
    let mut a: u64 = 0;
    let mut b: u64 = 1;
    for _ in 1..n { let temp = b; b = a + b; a = temp; }
    b
}

fn main() {
    for i in 0..10 { println(`fib({i}) = {fib_iter(i)}`); }
}
```

### 12.3 Linked List with Enums and ARC

```nudl
enum List<T> {
    Cons { head: T, tail: List<T> },
    Nil,
}

impl<T: Printable> List<T> {
    fn new() -> List<T> { List::Nil }

    fn prepend(self, value: T) -> List<T> {
        List::Cons { head: value, tail: self }
    }

    fn len(self) -> u64 {
        match self {
            List::Cons { tail, .. } => 1 + tail.len(),
            List::Nil => 0,
        }
    }
}

fn main() {
    let list = List::new().prepend(3).prepend(2).prepend(1);
    println(`Length: {list.len()}`);  // Length: 3
}
```

Because `List` is an enum (a reference type), `self` in `prepend` shares the existing list via
ARC. The tail of the new `Cons` node points to the same allocation -- no deep copy occurs.

### 12.4 Iterator Pipeline with Chaining

`filter`, `map`, and `fold` are lazy iterator methods that return adapter iterators
(`FilterIterator<T>`, `MapIterator<T, U>`, etc.). Use `.collect()` to materialize results
into a collection, or `.fold()` to reduce to a single value.

```nudl
fn main() {
    let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

    // Lazy iterator chain: nothing executes until .fold() drives the iteration
    let sum = data.iter()
        .filter(|x| x % 2 == 0)
        .map(|x| x * 2)
        .fold(0) { acc, x -> acc + x };
    println(`Sum: {sum}`);  // Sum: 60

    // Collect into a new array
    let evens: i32[] = data.iter()
        .filter(|x| x % 2 == 0)
        .collect();
    println(`Evens: {evens}`);  // Evens: [2, 4, 6, 8, 10]
}
```

### 12.5 Comptime Code Generation

```nudl
comptime fn make_vector_type(comptime n: u32) {
    let names = ["x", "y", "z", "w"];
    let fields = names[0..n];
    let type_name = `Vec{n}`;
    quote {
        struct ${type_name} {
            ${for name in fields { quote { ${name}: f64, } }}
        }
    }
}

comptime {
    make_vector_type(2);  // generates Vec2 { x: f64, y: f64 }
    make_vector_type(3);  // generates Vec3 { x: f64, y: f64, z: f64 }
}

fn main() {
    let a = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
    let b = Vec3 { x: 4.0, y: 5.0, z: 6.0 };
    println(`({a.x + b.x}, {a.y + b.y}, {a.z + b.z})`);
    // (5.0, 7.0, 9.0)
}
```

### 12.6 Error Handling with Result and ?

```nudl
enum ParseError {
    InvalidFormat(string),
    OutOfRange { value: i64, min: i64, max: i64 },
    EmptyInput,
}

fn parse_port(input: string) -> Result<u16, ParseError> {
    if input.len() == 0 { return Err(ParseError::EmptyInput); }
    let value = parse_i64(input).map_err(|e| ParseError::InvalidFormat(e.to_string()))?;
    if value < 0 || value > 65535 {
        return Err(ParseError::OutOfRange { value, min: 0, max: 65535 });
    }
    Ok(value as u16)
}

fn main() {
    match parse_port("8080") {
        Ok(port) => println(`Port: {port}`),
        Err(e) => println(`Error: {e.to_string()}`),
    }
}
```

### 12.7 Concurrent Data Fetcher

```nudl
async fn fetch_page(url: string) -> Result<string, Error> {
    let response = http_get(url).await?;
    if response.status() != 200 {
        return Err(Error::new(`HTTP {response.status()} for {url}`));
    }
    Ok(response.body())
}

async fn fetch_all(urls: string[]) -> Result<string[], Error> {
    let results = Task.group { group ->
        for url in urls {
            group.spawn(async { fetch_page(url).await });
        }
    };
    // Collect results, propagating any errors
    let mut pages: string[] = [];
    for result in results {
        pages.push(result?);
    }
    Ok(pages)
}

async fn main() {
    let urls = [
        "https://example.com/page1",
        "https://example.com/page2",
        "https://example.com/page3",
    ];
    match fetch_all(urls).await {
        Ok(pages) => {
            for (i, page) in pages.enumerate() {
                println(`Page {i + 1}: {page.len()} bytes`);
            }
        },
        Err(e) => println(`Error: {e.to_string()}`),
    }
}
```

### 12.8 Pipeline with Pipe Operator

```nudl
fn tokenize(input: string) -> string[] {
    input.split(" ")
}

fn remove_stopwords(words: string[]) -> string[] {
    let stops = ["the", "a", "is", "in", "of"];
    filter(words) { !stops.contains(it) }
}

fn to_lowercase(words: string[]) -> string[] {
    map(words) { it.to_lower() }
}

fn word_count(words: string[]) -> Map<string, i32> {
    let mut counts: Map<string, i32> = Map::new();
    for word in words {
        let current = counts.get(word).unwrap_or(0);
        counts.insert(word, current + 1);
    }
    counts
}

fn main() {
    let text = "The quick brown fox jumps over the lazy dog";

    // Pipeline reads top-to-bottom
    let counts = text
        |> tokenize
        |> to_lowercase
        |> remove_stopwords
        |> word_count;

    for (word, count) in counts {
        println(`{word}: {count}`);
    }
}
```

---

## 13. Key Design Decisions Explained

### 13.1 Why ARC Instead of a Borrow Checker?

The borrow checker is Rust's most powerful feature and also its steepest learning curve. nudl
targets developers who want Rust-level expressiveness without the cognitive overhead of lifetime
annotations. ARC provides deterministic cleanup, predictable performance, and a simpler mental
model. The trade-off is the possibility of reference cycles, mitigated by `weak` references and
compile-time cycle detection warnings.

### 13.2 Why Interfaces Instead of Traits?

nudl interfaces do not support associated types, eliminating an entire category of type-level
complexity. Generic parameters go on the interface instead:

```nudl
// Rust:  trait Iterator { type Item; fn next(&mut self) -> Option<Self::Item>; }
// nudl:
interface Iterator<T> { fn next(mut self) -> Option<T>; }
```

### 13.3 Why Comptime Instead of Macros?

Procedural macros operate on token streams -- a split world where macro code behaves differently
from regular code. nudl follows Zig: `comptime` blocks are regular nudl code executed in a
sandboxed VM. Types are values you can inspect, compose, and emit. No separate macro language.

### 13.4 Why Named Arguments?

Named arguments make code self-documenting, especially with multiple same-typed parameters:

```nudl
// Unclear:
configure(true, false, true);
// Clear:
configure("server", verbose: true, debug: false, tls: true);
```

The first-parameter exemption keeps common patterns concise.

### 13.5 Why No String Slices?

Rust's `&str`/`String` duality causes friction. nudl has a single `string` type: UTF-8, ARC'd,
immutable content. Substring operations allocate new strings. Passing strings is just a refcount
increment.

### 13.6 Why Allow Aliased Mutation?

Multiple `mut` bindings can refer to the same allocation -- the same model as Swift and Java. The
trade-off is the loss of data-race safety at compile time. nudl targets single-threaded and
cooperative-concurrency use cases.

---

## 14. Syntax Quick Reference

```nudl
let x = 42;                                   // immutable binding
let mut y = 0;                                 // mutable binding
let z: f64 = 3.14;                             // explicit type annotation
const MAX: u32 = 1024;                         // compile-time constant

fn add(a: i32, b: i32) -> i32 { a + b }       // function (expression body)
struct Point { x: f64, y: f64 }                // struct
enum Option<T> { Some(T), None }               // enum (ADT)
interface Printable { fn to_string(self) -> string; }

if cond { a } else { b }                       // if expression
if let Some(v) = opt { use(v); }               // if let
match x { 0 => "zero", n => `{n}` }             // match expression
for item in list { process(item); }            // for-in loop
'label: for item in list { break 'label; }     // labeled loop
while cond { step(); }                         // while loop
while let Some(v) = iter.next() { use(v); }    // while let
loop { if done { break result; } }             // infinite loop with break
type Alias = ExistingType;                     // type alias

let value = might_fail()?;                     // error propagation
panic("unrecoverable");                        // abort

let f = |x: i32| x * 2;                       // closure
let evens = filter(list) { it % 2 == 0 };     // trailing lambda
defer { cleanup(); }                           // deferred execution
let combined = [...a, ...b];                   // array spread
let modified = Config { ...base, port: 443 };  // struct spread

import std::collections::Map;                  // import
pub fn public_function() -> i32 { 42 }         // public visibility
comptime { generate_code(); }                  // compile-time block
quote { fn ${name}() -> ${T} { ... } }        // comptime code generation
#[key = "value", flag]                         // attributes
let msg = `Result: {compute()}`;               // string interpolation
let weak r = strong_ref;                       // weak reference

async fn fetch(url: string) -> string { ... } // async function
let data = fetch(url).await;                  // postfix await
await long_computation()                       // prefix await
let f = async { compute().await };            // async block
let h = Task.spawn(async { work().await });   // spawn task
actor Counter { value: i32, ... }             // actor type

let result = data |> transform |> validate;   // pipe operator

#[derive(Clone, Eq, Printable)]                // derive
struct Config { port: u16 = 8080 }             // field defaults
let r = 0..10;                                 // range type
fn exit(code: i32) -> ! { ... }               // never type
break 'label value;                            // labeled break with value
extern { fn printf(fmt: CStr, ...) -> i32; }      // variadic FFI
```

### 14.1 Destructuring Patterns

Destructuring is supported in all pattern contexts:

```nudl
// Let bindings
let (x, y) = get_point();
let Point { x, y } = point;

// For loops
for (key, value) in map {
    println(`{key}: {value}`);
}

// Match arms
match result {
    Ok((first, second)) => println(`{first}, {second}`),
    Err(e) => println(`Error: {e.message()}`),
}

// Closure parameters
let distances = map(points) { |(x, y)| sqrt(x * x + y * y) };

// Trailing lambda with destructuring
let labels = map(entries) { (name, count) -> `{name}: {count}` };
```

### 14.2 Operator Precedence

| Prec | Category       | Operators                              | Assoc  |
|------|----------------|----------------------------------------|--------|
| 15   | Primary        | `.` `[]` `()`                          | Left   |
| 14   | Postfix        | `?` `.await`                           | Postfix|
| 13   | Prefix         | `-` (unary) `!`                        | Prefix |
| 12   | Cast           | `as`                                   | Left   |
| 11   | Multiplicative | `*` `/` `%`                            | Left   |
| 10   | Additive       | `+` `-`                                | Left   |
| 9    | Shift          | `<<` `>>`                              | Left   |
| 8    | Bitwise AND    | `&`                                    | Left   |
| 7    | Bitwise XOR    | `^`                                    | Left   |
| 6    | Bitwise OR     | `\|`                                   | Left   |
| 5    | Comparison     | `==` `!=` `<` `>` `<=` `>=`           | None   |
| 4    | Logical AND    | `&&`                                   | Left   |
| 3    | Logical OR     | `\|\|`                                 | Left   |
| 2    | Range          | `..` `..=`                             | None   |
| 1    | Pipe           | `\|>`                                  | Left   |
| 0    | Assignment     | `=` `+=` `-=` `*=` `/=` `%=`          | Right  |

Comparison operators are non-chaining: `a < b < c` is a syntax error. Prefix `await` and `return`
are keyword expressions, not operators.
