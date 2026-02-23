## 20. Standard Library (v1)

The nudl v1 standard library provides a minimal but complete foundation for
general-purpose programming. It covers I/O, error handling, collections,
string manipulation, and basic math.

### 20.1 Core Interfaces

#### 20.1.1 Error

```nudl
interface Error {
    fn message(self) -> string;
}
```

All error types used as the `E` parameter in `Result<T, E>` must implement
`Error`. This provides a uniform way to extract human-readable error messages.
See Section 15.6.

#### 20.1.2 From (Type Conversion)

```nudl
interface From<Source, Target> {
    fn from(source: Source) -> Target;
}
```

Used for explicit type conversions and by the `?` operator for automatic error
type conversion. See Section 15.7.

### 20.2 Option Methods

`Option<T>` provides the following methods:

| Method | Signature | Description |
|---|---|---|
| `is_some` | `fn is_some(self) -> bool` | Returns `true` if `Some` |
| `is_none` | `fn is_none(self) -> bool` | Returns `true` if `None` |
| `unwrap` | `fn unwrap(self) -> T` | Returns the inner value; panics if `None` |
| `unwrap_or` | `fn unwrap_or(self, default: T) -> T` | Returns inner value or the default |
| `map` | `fn map<U>(self, transform: (T) -> U) -> Option<U>` | Applies transform if `Some` |
| `and_then` | `fn and_then<U>(self, f: (T) -> Option<U>) -> Option<U>` | Chains fallible operations |
| `ok_or` | `fn ok_or<E: Error>(self, err: E) -> Result<T, E>` | Converts to `Result`, using `err` for `None` |

```nudl
let name: Option<string> = get_name();
let upper = name.map { it.to_upper() };
let len = name.map { it.len() }.unwrap_or(0);
let result = name.ok_or(AppError { code: 404, detail: "not found" });
```

### 20.3 Result Methods

`Result<T, E>` provides the following methods (where `E: Error`):

| Method | Signature | Description |
|---|---|---|
| `is_ok` | `fn is_ok(self) -> bool` | Returns `true` if `Ok` |
| `is_err` | `fn is_err(self) -> bool` | Returns `true` if `Err` |
| `unwrap` | `fn unwrap(self) -> T` | Returns the `Ok` value; panics if `Err` |
| `unwrap_or` | `fn unwrap_or(self, default: T) -> T` | Returns `Ok` value or the default |
| `map` | `fn map<U>(self, transform: (T) -> U) -> Result<U, E>` | Transforms the `Ok` value |
| `map_err` | `fn map_err<F: Error>(self, transform: (E) -> F) -> Result<T, F>` | Transforms the `Err` value |
| `and_then` | `fn and_then<U>(self, f: (T) -> Result<U, E>) -> Result<U, E>` | Chains fallible operations |
| `ok` | `fn ok(self) -> Option<T>` | Converts to `Option`, discarding error |
| `err` | `fn err(self) -> Option<E>` | Extracts the error, if present |

```nudl
let result: Result<i32, AppError> = parse_number(input);
let doubled = result.map { it * 2 };
let chained = result.and_then { validate(it) };
```

### 20.4 Set Type

`Set<T>` is a hash set of unique values. `T` must implement `Eq`.

```nudl
let mut s: Set<string> = Set::new();
s.insert("hello");
s.insert("world");
s.insert("hello");    // no-op, already present
println(f"{s.len()}");  // 2
```

| Method | Signature | Description |
|---|---|---|
| `new` | `fn new() -> Set<T>` | Creates an empty set |
| `insert` | `fn insert(mut self, value: T) -> bool` | Inserts value; returns `true` if new |
| `remove` | `fn remove(mut self, value: T) -> bool` | Removes value; returns `true` if present |
| `contains` | `fn contains(self, value: T) -> bool` | Checks membership |
| `len` | `fn len(self) -> u64` | Number of elements |
| `is_empty` | `fn is_empty(self) -> bool` | Returns `true` if empty |
| `union` | `fn union(self, other: Set<T>) -> Set<T>` | Set union |
| `intersection` | `fn intersection(self, other: Set<T>) -> Set<T>` | Set intersection |
| `difference` | `fn difference(self, other: Set<T>) -> Set<T>` | Elements in self but not other |

`Set<T>` is a reference type (heap-allocated, ARC'd) and implements
`Iterator<T>` for use in `for` loops.

Note: nudl v1 does not expose a separate `Hash` interface. The runtime uses a built-in hashing mechanism for types that implement `Eq`. Custom hash behavior is planned for v2.

### 20.5 Map Type

`Map<K, V>` is a hash map from keys of type `K` to values of type `V`. `K` must implement `Eq`.

```nudl
let mut scores: Map<string, i32> = Map::new();
scores.insert("alice", 100);
scores.insert("bob", 85);
let value = scores.get("alice");   // Option<i32>
```

| Method | Signature | Description |
|---|---|---|
| `new` | `fn new() -> Map<K, V>` | Creates an empty map |
| `insert` | `fn insert(mut self, key: K, value: V) -> Option<V>` | Inserts key-value pair; returns previous value if key existed |
| `get` | `fn get(self, key: K) -> Option<V>` | Returns the value for a key, or `None` |
| `remove` | `fn remove(mut self, key: K) -> Option<V>` | Removes a key; returns the value if present |
| `contains_key` | `fn contains_key(self, key: K) -> bool` | Checks if a key is present |
| `len` | `fn len(self) -> u64` | Number of key-value pairs |
| `is_empty` | `fn is_empty(self) -> bool` | Returns `true` if empty |
| `keys` | `fn keys(self) -> Iterator<K>` | Iterates over keys |
| `values` | `fn values(self) -> Iterator<V>` | Iterates over values |

`Map<K, V>` is a reference type (heap-allocated, ARC'd) and implements `Iterator<(K, V)>`, yielding key-value pairs as tuples. Iteration order is not guaranteed.

Note: nudl v1 does not expose a separate `Hash` interface. The runtime uses a built-in hashing mechanism for types that implement `Eq`. Custom hash behavior is planned for v2.

### 20.6 Math Functions

The `std::math` module provides basic mathematical operations:

| Function | Signature | Description |
|---|---|---|
| `abs` | `fn abs<T: Ord + Neg<T>>(x: T) -> T` | Absolute value |
| `min` | `fn min<T: Ord>(a: T, b: T) -> T` | Minimum of two values |
| `max` | `fn max<T: Ord>(a: T, b: T) -> T` | Maximum of two values |
| `sqrt` | `fn sqrt(x: f64) -> f64` | Square root |
| `pow` | `fn pow(base: f64, exp: f64) -> f64` | Exponentiation |
| `floor` | `fn floor(x: f64) -> f64` | Floor (round toward negative infinity) |
| `ceil` | `fn ceil(x: f64) -> f64` | Ceiling (round toward positive infinity) |
| `round` | `fn round(x: f64) -> f64` | Round to nearest integer |
| `sin` | `fn sin(x: f64) -> f64` | Sine (radians) |
| `cos` | `fn cos(x: f64) -> f64` | Cosine (radians) |
| `tan` | `fn tan(x: f64) -> f64` | Tangent (radians) |
| `log` | `fn log(x: f64) -> f64` | Natural logarithm |
| `log2` | `fn log2(x: f64) -> f64` | Base-2 logarithm |

Math functions that operate on `f64` follow IEEE 754 semantics: `sqrt(-1.0)`
returns `NaN`, `log(0.0)` returns negative infinity, etc.

### 20.7 String Methods

The `string` type provides the following methods. All methods that produce a
modified string allocate a new string (strings are immutable).

| Method | Signature | Description |
|---|---|---|
| `len` | `fn len(self) -> u64` | Byte length of the string |
| `is_empty` | `fn is_empty(self) -> bool` | Returns `true` if length is zero |
| `contains` | `fn contains(self, needle: string) -> bool` | Substring search |
| `starts_with` | `fn starts_with(self, prefix: string) -> bool` | Prefix test |
| `ends_with` | `fn ends_with(self, suffix: string) -> bool` | Suffix test |
| `split` | `fn split(self, separator: string) -> string[]` | Split into parts |
| `join` | `fn join(self: string[], separator: string) -> string` | Join array elements with separator (`parts.join(", ")`) |
| `trim` | `fn trim(self) -> string` | Remove leading/trailing whitespace |
| `to_upper` | `fn to_upper(self) -> string` | Convert to uppercase |
| `to_lower` | `fn to_lower(self) -> string` | Convert to lowercase |
| `substring` | `fn substring(self, start: u64, end: u64) -> Option<string>` | Byte-range substring (see below) |
| `chars` | `fn chars(self) -> Iterator<char>` | Iterate over Unicode scalar values |
| `bytes` | `fn bytes(self) -> Iterator<u8>` | Iterate over raw bytes |
| `replace` | `fn replace(self, old: string, new: string) -> string` | Replace all occurrences |
| `repeat` | `fn repeat(self, count: u64) -> string` | Repeat string N times |

```nudl
let greeting = "Hello, World!";
let upper = greeting.to_upper();           // "HELLO, WORLD!"
let parts = greeting.split(", ");           // ["Hello", "World!"]
let repeated = "ha".repeat(3);              // "hahaha"
```

`substring(start: u64, end: u64) -> Option<string>` — extracts a byte-range substring. Indices are clamped to the valid range `[0, len]`. If the clamped range is empty (start >= end after clamping), returns `None`. Otherwise returns `Some(new_string)`.

```nudl
"hello".substring(0, 5)    // Some("hello")
"hello".substring(0, 100)  // Some("hello") — clamped to len
"hello".substring(3, 3)    // None — empty range
"hello".substring(10, 20)  // None — clamped range is empty
```

If the clamped range splits a multi-byte UTF-8 character, the behavior is a runtime panic (byte indices must fall on character boundaries).

### 20.8 I/O

The v1 standard library provides basic synchronous file I/O through the
`std::io` module. All I/O operations return `Result` types for error handling.

#### 20.8.1 Simple File Operations

| Function | Signature | Description |
|---|---|---|
| `read_file` | `fn read_file(path: string) -> Result<string, IoError>` | Read entire file as UTF-8 string |
| `write_file` | `fn write_file(path: string, content: string) -> Result<(), IoError>` | Write string to file (creates/overwrites) |
| `file_exists` | `fn file_exists(path: string) -> bool` | Check if a file exists |

```nudl
import std::io::{read_file, write_file, file_exists};

fn main() {
    if file_exists("config.toml") {
        match read_file("config.toml") {
            Ok(content) => println(f"Config: {content.len()} bytes"),
            Err(e) => println(f"Error: {e.message()}"),
        }
    }

    write_file("output.txt", content: "Hello, file!").unwrap();
}
```

#### 20.8.2 Streaming I/O

For large files or incremental processing, streaming reader and writer types
are provided:

```nudl
import std::io::{FileReader, FileWriter};

fn copy_file(src: string, dst: string) -> Result<(), IoError> {
    let reader = FileReader::open(src)?;
    defer { reader.close(); }

    let writer = FileWriter::create(dst)?;
    defer { writer.close(); }

    while let Some(chunk) = reader.read_chunk(4096)? {
        writer.write(chunk)?;
    }
    Ok(())
}
```

#### 20.8.3 IoError

```nudl
struct IoError {
    kind: IoErrorKind,
    detail: string,
}

enum IoErrorKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    Other,
}

impl Error for IoError {
    fn message(self) -> string {
        f"{self.kind}: {self.detail}"
    }
}
```

### 20.9 Print Functions

```nudl
fn print(value: dyn Printable)
fn println(value: dyn Printable)
```

`print` writes the string representation of `value` to standard output.
`println` does the same followed by a newline character (`\n`).

The argument must implement the `Printable` interface. String interpolation
expressions (`f"..."`) produce `string` values, which implement `Printable`.

```nudl
print("no newline");
println(f"value = {x}");
println(42);              // integers implement Printable
println(true);            // bools implement Printable
```

All primitive types (`i8`..`i64`, `u8`..`u64`, `f32`, `f64`, `bool`, `char`,
`string`, `()`) implement `Printable`. User-defined types must implement it
explicitly or via comptime derivation.

### 20.10 Iterator Extensions

The `Iterator<T>` interface (see Section 15.4) provides extension methods
available on all iterators. The following adapter types are returned by these
methods:

**`EnumerateIterator<T>`** implements `Iterator<(u64, T)>`, yielding
`(index, element)` tuples where the index starts at 0.

```nudl
let names = ["alice", "bob", "carol"];
for (i, name) in names.enumerate() {
    println(f"{i}: {name}");
}
// 0: alice
// 1: bob
// 2: carol
```

### 20.11 Standard Library Derive Functions

The standard library provides comptime derive functions for core interfaces (see Section 13):

| Function | Interface | Behavior |
|---|---|---|
| `derive_clone(T)` | `Clone` | Clones each field |
| `derive_eq(T)` | `Eq` | Compares all fields for equality |
| `derive_ord(T)` | `Ord` | Compares fields in declaration order (lexicographic) |
| `derive_printable(T)` | `Printable` | Formats as `"TypeName { field1: value1, field2: value2 }"` |
| `derive_drop(T)` | `Drop` | Generates `Drop` by dropping all fields in declaration order. Usually not needed — the compiler inserts a default drop automatically. |

These are invoked via the `#[derive(...)]` attribute:

```nudl
#[derive(Clone, Eq, Ord, Printable)]
struct Point { x: f64, y: f64 }
```

A derive function requires that all fields of the target type implement the derived interface. If a field type does not implement the interface, the derive emits a compile error with a clear message identifying the offending field.

---
