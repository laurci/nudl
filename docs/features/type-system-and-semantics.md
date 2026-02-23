## 5. Type System

### 5.1 Primitive Types

```
i8   i16  i32  i64       Signed integers
u8   u16  u32  u64       Unsigned integers
f32  f64                  Floating-point
bool                      Boolean (true, false)
char                      Unicode scalar value
string                    UTF-8, ARC'd, immutable content
()                        Unit type
```

The **never type `!`** can be used as an explicit return type for functions that never return (always
panic, infinite loop, process exit). `!` coerces to any type.

Integer arithmetic wraps on overflow using two's complement in all build modes.

### 5.2 Compound Types

```nudl
let point: (f64, f64) = (3.14, 2.71);         // tuple (value type)
let mut nums: i32[] = [1, 2, 3];               // dynamic array (reference type)
let matrix: [f64; 3] = [1.0, 0.0, 0.0];       // fixed-size array (value type)
let mut scores: Map<string, i32> = Map::new(); // map (reference type)
let transform: (i32) -> i32 = |x| x * 2;      // function type
```

**Range types** -- `Range<T>` and `RangeInclusive<T>` are first-class types produced by `0..10` and
`0..=10` expressions. They implement `Iterator<T>` for integer and char types, and can be stored in
variables, passed to functions, and returned.

**Floating-point semantics:** nudl follows IEEE 754 for all floating-point operations. `NaN`
propagates through arithmetic, `Inf` is a valid value, and `0.0 / 0.0` produces `NaN`. For
equality, `NaN != NaN` (as per IEEE 754). For ordering, `Ord` uses total ordering where `NaN`
sorts after `Inf`.

### 5.3 Structs and Enums

```nudl
struct Point { x: f64, y: f64 }   // named-field struct
struct Color(u8, u8, u8);          // tuple struct
struct Marker;                      // unit struct

enum Shape {
    Circle(f64),                             // data variant
    Rectangle { width: f64, height: f64 },   // struct variant
    Point,                                    // unit variant
}
```

Struct fields may have **default values**: `struct Config { port: u16 = 8080 }`. Fields with defaults
can be omitted during construction.

Enum variants can be used as constructor functions: `Some` has type `(T) -> Option<T>`.

### 5.4 Generics

Generics use monomorphization -- each instantiation generates a specialized version:

```nudl
fn min<T: Ord>(a: T, b: T) -> T { if a < b { a } else { b } }

struct Pair<A, B> { first: A, second: B }

enum Result<T, E> { Ok(T), Err(E) }
```

All generic type parameters are **invariant** -- `Cat[]` cannot be used where `Animal[]` is
expected, even if `Cat` implements `Animal`. Use `(dyn Animal)[]` for polymorphic collections.

### 5.5 Interfaces

Interfaces define shared behavior. Unlike Rust traits, they have no associated types -- generic
parameters go on the interface itself:

```nudl
interface Printable {
    fn to_string(self) -> string;
}

interface Iterator<T> {
    fn next(mut self) -> Option<T>;
}

interface Index<Idx, Output> {
    fn index(self, idx: Idx) -> Output;
}

impl Printable for Point {
    fn to_string(self) -> string { f"({self.x}, {self.y})" }
}
```

Iterators are **fused** -- once `next()` returns `None`, all subsequent calls continue returning
`None`.

**Method resolution:** Inherent methods (defined in `impl Type`) win over interface methods. Use
qualified syntax to disambiguate: `Printable::to_string(obj)`.

### 5.6 Dynamic Dispatch

Use `dyn Interface` for runtime polymorphism:

```nudl
fn print_all(items: (dyn Printable)[]) {
    for item in items {
        println(item.to_string());
    }
}
```

---

## 6. Functions and Methods

### 6.1 Function Declarations

```nudl
fn add(a: i32, b: i32) -> i32 {
    a + b  // no semicolon = return value
}

fn greet(name: string) {
    println(f"Hello, {name}!");  // return type omitted = ()
}
```

### 6.2 Methods

Methods are defined in `impl` blocks with `self` or `mut self` receivers:

```nudl
struct Counter { value: i32 }

impl Counter {
    fn new() -> Counter { Counter { value: 0 } }
    fn get(self) -> i32 { self.value }
    fn increment(mut self) { self.value = self.value + 1; }
}
```

The `self` parameter is invisible to the calling convention -- it does not count as the "first
parameter" for the named-argument rule.

### 6.3 Closures

Closures capture variables **by ARC reference** -- mutations inside the closure are visible outside,
and vice versa. Value types are boxed when captured. There is no `move` keyword; all captures share
the same ARC-managed environment.

```nudl
fn make_adder(n: i32) -> (i32) -> i32 {
    |x| x + n
}

let add5 = make_adder(5);
println(f"{add5(3)}");  // prints 8
```

---

## 7. Control Flow

All control flow constructs are expressions that produce values.

### 7.1 if/else

```nudl
let status = if score >= 90 { "excellent" } else { "needs work" };
```

### 7.2 match

`match` requires exhaustive coverage. The compiler rejects incomplete matches.

```nudl
match value {
    0 => "zero",                       // literal
    1 | 2 | 3 => "small",             // or-pattern
    4..=10 => "medium",                // range
    n if n < 0 => f"negative: {n}",    // guard
    n => f"large: {n}",                // binding
}

// Struct patterns
match config {
    Config { tls: true, port: 443, .. } => "standard HTTPS",
    Config { tls: true, port, .. } => f"HTTPS on port {port}",
    Config { host, .. } => f"plaintext to {host}",
}
```

### 7.3 If Let and While Let

For single-pattern convenience without a full `match`:

```nudl
if let Some(value) = maybe_value {
    println(f"Got: {value}");
}

while let Some(item) = iter.next() {
    process(item);
}
```

### 7.4 Loops

```nudl
for item in collection { process(item); }
while condition { step(); }

let result = loop {
    let value = try_something();
    if value > threshold { break value; }
};
```

### 7.5 Labeled Loops

Labels allow breaking or continuing an outer loop from a nested loop:

```nudl
'outer: for row in grid {
    for cell in row {
        if cell == target { break 'outer; }
    }
}
```

When a `for` loop has a labeled `break value` path but may also exhaust normally, the loop
expression type is `Option<T>` -- `break 'label value` produces `Some(value)`, and normal
exhaustion produces `None`:

```nudl
let found: Option<i32> = for x in items 'search {
    if x > threshold {
        break 'search x;  // produces Some(x)
    }
};
// found is None if no item exceeded threshold
```

---

## 8. Error Handling

nudl uses `Result<T, E>` and `Option<T>` as built-in enums. There are no exceptions.

```nudl
fn read_config(path: string) -> Result<Config, Error> {
    let content = read_file(path)?;     // propagates Err early
    let parsed = parse_toml(content)?;
    Ok(parsed)
}
```

For unrecoverable situations: `panic("message")`.

All error types in `Result<T, E>` must implement the `Error` interface. The `?` operator uses the
`From` interface for automatic error type conversion when the inner and outer error types differ.
The `?` operator applies **one direct `From` conversion** only -- no transitive chaining. If no
direct `From<A, B>` exists, it is a compile error.

The `?` operator also works on `Option<T>`: if the value is `Some(v)`, it evaluates to `v`; if
`None`, it returns `None` from the enclosing function (which must return `Option<_>`).

---

## 9. Modules and Visibility

Each `.nudl` file is a module. Directories form namespaces with `mod.nudl`.

```
project/
  main.nudl
  math/
    mod.nudl          -- declares the math module
    vector.nudl       -- math::vector
    matrix.nudl       -- math::matrix
```

```nudl
import std::collections::Map;
import math::{Vector, Matrix};
import math::vector::Vector3 as Vec3;

pub struct Point { pub x: f64, pub y: f64 }   // public
fn helper() -> f64 { 0.0 }                     // private (default)

type StringList = string[];                     // type alias
```

---

## 10. Standard Library (v1)

### 10.1 Built-in Functions

```nudl
print("no newline");
println(f"interpolated: {value}");
```

### 10.2 Core Interfaces

`Clone`, `Drop`, `Printable`, `Iterator<T>`, and operator interfaces (`Add<Rhs, Output>`,
`Sub<Rhs, Output>`, `Mul<Rhs, Output>`, `Div<Rhs, Output>`, `Rem<Rhs, Output>`, `Neg<Output>`,
`Not<Output>`, `Eq`, `Ord`, `Index<Idx, Output>`, `IndexMut<Idx, Output>`).

### 10.3 Collections and Strings

```nudl
// Dynamic arrays
let mut list: i32[] = [1, 2, 3];
list.push(4);

// Maps
let mut table: Map<string, i32> = Map::new();
table.insert("key", 42);
let value = table.get("key");  // Option<i32>

// String operations (always allocate new strings)
let upper = greeting.to_upper();
let sub = greeting.substring(0, 5).unwrap();  // "Hello"
let parts = greeting.split(", ");

// String interpolation
let msg = f"Welcome to {name} v{version}!";
```

### 10.4 Error and From Interfaces

```nudl
interface Error { fn message(self) -> string; }
interface From<Source, Target> { fn from(source: Source) -> Target; }
```

All error types in `Result<T, E>` must implement `Error`. The `From` interface enables automatic
error conversion with the `?` operator.

### 10.5 Set

```nudl
let mut tags: Set<string> = Set::new();
tags.insert("rust");
tags.insert("nudl");
println(f"{tags.len()}");  // 2
```

### 10.6 Option and Result Methods

Both `Option<T>` and `Result<T, E>` provide `map`, `and_then`, `unwrap`, `unwrap_or`, and
type-checking methods (`is_some`/`is_none`, `is_ok`/`is_err`). `Option` also has `ok_or` to
convert to `Result`. `Result` has `map_err`, `ok`, and `err`.

### 10.7 Math

The `std::math` module provides: `abs`, `min`, `max`, `sqrt`, `pow`, `floor`, `ceil`, `round`,
`sin`, `cos`, `tan`, `log`, `log2`.

### 10.8 File I/O

Basic synchronous file operations in `std::io`:

```nudl
import std::io::{read_file, write_file, file_exists};

let content = read_file("data.txt")?;    // Result<string, IoError>
write_file("out.txt", content: data)?;    // Result<(), IoError>
let exists = file_exists("config.toml");  // bool
```

Streaming I/O is available via `FileReader` and `FileWriter` for large files.

---
