## 3. Type System

### 3.1 Primitive Types

nudl provides the following primitive types:

#### 3.1.1 Integer Types

| Type  | Size    | Range                                        |
|-------|---------|----------------------------------------------|
| `i8`  | 1 byte  | -128 to 127                                  |
| `i16` | 2 bytes | -32,768 to 32,767                            |
| `i32` | 4 bytes | -2,147,483,648 to 2,147,483,647              |
| `i64` | 8 bytes | -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807 |
| `u8`  | 1 byte  | 0 to 255                                     |
| `u16` | 2 bytes | 0 to 65,535                                  |
| `u32` | 4 bytes | 0 to 4,294,967,295                           |
| `u64` | 8 bytes | 0 to 18,446,744,073,709,551,615              |

Integer arithmetic that overflows wraps using two's complement in all build
modes. There is no distinction between debug and release builds for overflow
behavior — wrapping is always the defined semantics.

#### 3.1.1a Integer Division and Remainder

Integer division truncates toward zero: `-7 / 2 == -3`. The remainder operator
follows: `-7 % 2 == -1`. This is consistent with C, Rust, Java, and Swift.

Integer division by zero causes a runtime panic with the message "division by
zero". This applies to both `/` and `%` operators.

#### 3.1.2 Floating-Point Types

| Type  | Size    | Precision | Standard      |
|-------|---------|-----------|---------------|
| `f32` | 4 bytes | ~7 digits | IEEE 754 binary32 |
| `f64` | 8 bytes | ~15 digits| IEEE 754 binary64 |

#### 3.1.2a Floating-Point Semantics

nudl follows **IEEE 754** for all floating-point operations on `f32` and `f64`:

- **NaN propagation.** Any arithmetic operation where one or both operands are
  `NaN` produces `NaN`. This includes `NaN + 1.0`, `NaN * 0.0`, etc.
- **Infinity.** `Inf` and `-Inf` are valid values. `1.0 / 0.0` produces `Inf`.
  `-1.0 / 0.0` produces `-Inf`. `Inf + Inf` produces `Inf`.
- **Division by zero.** `0.0 / 0.0` produces `NaN`. Non-zero divided by zero
  produces `Inf` or `-Inf` depending on signs.
- **Negative zero.** `-0.0` is a valid value. `-0.0 == 0.0` is `true`.

**Equality (`Eq` for floats):** Float types implement `Eq` following IEEE 754
semantics: `NaN != NaN` (comparing `NaN` to anything, including itself, returns
`false` for `==` and `true` for `!=`).

**Ordering (`Ord` for floats):** Float types implement `Ord` using a **total
ordering** that extends IEEE 754 comparison: `NaN` sorts after `Inf` (i.e.,
`NaN` is considered greater than all non-`NaN` values, including `Inf`). When
both operands are `NaN`, `cmp` returns `Ordering::Equal`. This total ordering
ensures that sorting algorithms produce deterministic results even with `NaN`
values present.

#### 3.1.3 Boolean Type

The type `bool` has exactly two values: `true` and `false`. It occupies 1 byte.

#### 3.1.4 Character Type

The type `char` represents a Unicode scalar value. It occupies 4 bytes and can
hold any value in the range U+0000 to U+D7FF or U+E000 to U+10FFFF.

#### 3.1.5 String Type

The type `string` represents an immutable, UTF-8-encoded string. Strings are
reference types: they are heap-allocated and reference-counted. Assignment of a
string increments the reference count; the underlying bytes are not copied.

All operations that produce a substring or modified string allocate a new string.
There are no string slices or string references.

`string` implements the `Error` interface, making it valid as the error type in `Result<T, string>`.

**String comparison:** Two strings are equal if and only if their underlying
byte sequences are identical (byte-level comparison of UTF-8 encoded data). No
Unicode normalization is performed. Ordering (`Ord`) compares strings
lexicographically by byte value (UTF-8 byte order).

#### 3.1.6 Unit Type

The unit type `()` has exactly one value, also written `()`. It is the implicit
return type of functions that do not return a meaningful value. It is a value
type occupying zero bytes.

#### 3.1.7 FFI Types

nudl provides three built-in types for foreign function interface interop:

| Type | Size | Description | C equivalent (AArch64) |
|------|------|-------------|----------------------|
| `RawPtr` | 8 bytes | Opaque immutable pointer | `const void*` |
| `MutRawPtr` | 8 bytes | Opaque mutable pointer | `void*` |
| `CStr` | 8 bytes | Null-terminated C string pointer | `const char*` |

These are value types: assignment copies the pointer value (the address), not
the pointee data. They have no dereference or arithmetic operations — they are
opaque handles for passing between nudl and C code.

See Section 19 for the full FFI specification including operations, lifetime
rules, and restrictions.

#### 3.1.8 Never Type

The never type `!` represents computations that never produce a value — functions
that always panic, loop forever, or call `std::process::exit()`. It can be used
as an explicit return type:

```nudl
fn exit(code: i32) -> ! {
    // calls OS exit — never returns
}

fn infinite() -> ! {
    loop { process_events(); }
}
```

The never type coerces to any other type, which allows it to be used in any
expression position:

```nudl
let x: i32 = if condition { 42 } else { panic("unreachable") };
// panic returns !, which coerces to i32
```

### 3.2 Compound Types

#### 3.2.1 Tuples

```
tuple_type  = '(' type % ',' ')' ;
```

A tuple is an ordered, fixed-size, heterogeneous collection of values. Tuples
are value types: assignment copies all elements. Elements are accessed by
zero-based numeric index.

```nudl
let point: (f64, f64) = (3.14, 2.71);
let x = point.0;       // 3.14
let y = point.1;       // 2.71
```

A single-element tuple requires a trailing comma to distinguish it from
parenthesized expressions: `(i32,)` is a tuple type; `(i32)` is just `i32`.

The unit type `()` is the zero-element tuple.

A tuple is a value type if and only if all of its element types are value types.
If any element is a reference type, the entire tuple is heap-allocated and
ARC-managed (i.e., it becomes a reference type). Assignment and parameter passing
share the tuple via ARC, not by copying individual elements.

#### 3.2.2 Dynamic Arrays

```
dynamic_array_type  = type '[]' ;
```

A dynamic array `T[]` is a growable, ordered sequence of elements of type `T`.
Dynamic arrays are reference types: they are heap-allocated and
reference-counted. Assignment shares the underlying storage.

```nudl
let mut numbers: i32[] = [1, 2, 3];
numbers.push(4);
let alias = numbers;   // refcount++, shared storage
```

#### 3.2.3 Fixed-Size Arrays

```
fixed_array_type  = '[' type ';' expression ']' ;
```

A fixed-size array `[T; N]` contains exactly `N` elements of type `T`. `N` must
be a compile-time-known non-negative integer. Fixed-size arrays are value types:
they are stack-allocated and copied on assignment.

```nudl
let matrix: [f64; 3] = [1.0, 0.0, 0.0];
let copy = matrix;     // full copy
```

#### 3.2.4 Maps

```
map_type  = 'Map' '<' type ',' type '>' ;
```

`Map<K, V>` is a hash map from keys of type `K` to values of type `V`. Keys
must implement the `Eq` interface. Maps are reference types.

```nudl
let mut scores: Map<string, i32> = Map::new();
scores.insert("alice", 100);
let value = scores.get("alice");   // Option<i32>
```

#### 3.2.5 Function Types

```
function_type  = '(' type % ',' ')' '->' type ;
```

A function type describes the signature of a function or closure. Function types
are reference types (closures capture their environment on the heap).

```nudl
let transform: (i32) -> i32 = |x| x * 2;
let combine: (i32, i32) -> i32 = |a, b| a + b;
```

#### 3.2.6 Future Type

```
future_type  = 'Future' '<' type '>' ;
```

`Future<T>` represents a value of type `T` that will be available after an
asynchronous computation completes. Futures are reference types (heap-allocated
and ARC'd). They are not directly constructable by user code — they are produced
by `async fn` invocations and `async { }` blocks.

See Section 17 for the full async/await semantics.

#### 3.2.7 Range Types

`Range<T>` and `RangeInclusive<T>` are built-in generic types representing
half-open and closed ranges respectively. They are reference types.

```nudl
let r = 0..10;        // Range<i32>, half-open: [0, 10)
let ri = 0..=9;       // RangeInclusive<i32>, closed: [0, 9]
```

Range types implement `Iterator<T>` when `T` is an integer or `char` type. They
can be stored in variables, passed to functions, and returned:

```nudl
fn indices(n: i32) -> Range<i32> { 0..n }

let r = 0..5;
for i in r { ... }
```

Ranges also appear in pattern matching (Section 8) and for-loop iteration
(Section 10).

### 3.3 User-Defined Types

#### 3.3.1 Structs

Structs are product types with named fields, positional fields, or no fields.

```
struct_def        = 'struct' identifier generics? struct_body ;
struct_body       = '{' struct_field % ',' '}'       /* named-field struct   */
                  | '(' type % ',' ')' ';'           /* tuple struct         */
                  | ';'                               /* unit struct          */

struct_field      = visibility? identifier ':' type ( '=' expression )? ;
```

The optional `= expression` provides a default value for the field (see Section 5.3).

Structs are reference types. Assignment increments the reference count and shares
the allocation.

```nudl
struct Point { x: f64, y: f64 }                 // named-field struct
struct Color(u8, u8, u8);                        // tuple struct
struct Marker;                                    // unit struct
```

Tuple struct fields are accessed by numeric index:

```nudl
let c = Color(255, 128, 0);
let red = c.0;     // 255
```

Unit structs are constructed by name alone:

```nudl
let m = Marker;
```

#### 3.3.2 Enums

Enums are sum types. Each variant may carry data.

```
enum_def          = 'enum' identifier generics? '{' enum_variant % ',' '}' ;

enum_variant      = identifier                              /* unit variant   */
                  | identifier '(' type % ',' ')'           /* data variant   */
                  | identifier '{' struct_field % ',' '}'   /* struct variant */ ;
```

Enums are reference types.

```nudl
enum Shape {
    Circle(f64),
    Rectangle { width: f64, height: f64 },
    Point,
}
```

**Enum variants as constructors.** Data variants may be used as constructor
functions. The variant `Circle` has the type `(f64) -> Shape`. Unit variants
are values of their enum type. Struct variants are constructed with field syntax.

```nudl
let shapes: Shape[] = [
    Shape::Circle(5.0),
    Shape::Rectangle { width: 10.0, height: 20.0 },
    Shape::Point,
];

// Using a data variant as a function:
let radii = [1.0, 2.0, 3.0];
let circles = map(radii) { Shape::Circle(it) };
```

#### 3.3.3 Type Aliases

```
type_alias  = 'type' identifier generics? '=' type ';' ;
```

A type alias introduces an alternative name for an existing type. Aliases are
fully transparent: the alias and the original type are interchangeable in all
contexts.

```nudl
type StringList = string[];
type Pair<A, B> = (A, B);
type Result<T> = Result<T, string>;
```

#### 3.3.4 Actors

Actors are concurrent reference types with isolated mutable state. See Section
17.7 for full syntax and semantics.

```
actor_def  = visibility? 'actor' identifier generics? '{' actor_member* '}' ;
actor_member = struct_field | fn_def ;
```

Actors are reference types: they are heap-allocated and ARC'd. All method calls
from outside the actor are implicitly asynchronous and return `Future<T>`. Within
the actor's own methods, `self` access is synchronous.

#### 3.3.5 Recursive Types

Recursive type definitions are permitted because user-defined struct and enum
types are reference types (heap-allocated, ARC-managed). The self-referential
field is a pointer, not an inlined value, so the type has finite size.

```nudl
struct Node<T> {
    value: T,
    next: Option<Node<T>>,  // ARC pointer — finite size
}
```

A non-optional recursive field like `struct Node { next: Node }` is valid at
the type level but unconstructable — there is no base case. The compiler emits
a warning for unconstructable recursive types.

Weak self-references are also permitted and useful for breaking reference cycles:

```nudl
struct TreeNode {
    children: TreeNode[],
    weak parent: TreeNode,  // weak reference — breaks cycle
}
```

### 3.4 Type Inference

nudl employs Hindley-Milner-style bidirectional type inference. Types flow in
two directions:

- **Bottom-up (synthesis):** Literal values, variable references, and operator
  applications synthesize their types from their components.
- **Top-down (checking):** Annotations, function parameter types, and return
  types push expected types downward, constraining sub-expressions.

**When annotations are required:**

- **Function parameter types** must always be annotated. No inference is
  performed for function parameter types.
- **Function return types** must always be annotated. When the return type
  annotation is omitted, the return type defaults to `()` — the compiler does
  not infer return types from the function body.
- **Struct and enum field types** must always be annotated.
- **`let` bindings** may omit the type annotation if the initializer provides
  enough information for inference. This is the only position where type
  inference is fully automatic.

**Default types for unsuffixed literals:**

- An integer literal with no suffix and no constraining context defaults to `i32`.
- A float literal with no suffix and no constraining context defaults to `f64`.

**Type inference across let bindings:**

```nudl
let x = 42;                // inferred as i32
let y: u64 = 42;           // annotation constrains literal to u64
let z = 3.14;              // inferred as f64
let w = [1, 2, 3];         // inferred as i32[]
let v = (true, "hello");   // inferred as (bool, string)
```

**Type inference for closures:**

Closure parameter types may be omitted when the expected type provides them:

```nudl
let numbers: i32[] = [1, 2, 3];
let doubled = map(numbers) { it * 2 };   // it: i32 inferred from i32[]
```

### 3.5 Generics

#### 3.5.1 Generic Declarations

Functions, structs, enums, interfaces, and type aliases may be parameterized by
type variables.

```
generics        = '<' generic_param % ',' '>' ;
generic_param   = identifier ( ':' bounds )? ;
bounds          = bound ( '+' bound )* ;
bound           = path ;                     /* interface name */
```

```nudl
fn max<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

struct Pair<A, B> {
    first: A,
    second: B,
}

enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

#### 3.5.2 Where Clauses

For complex bounds, a `where` clause may be used instead of or in addition to
inline bounds:

```
where_clause  = 'where' where_pred % ',' ;
where_pred    = identifier ':' bounds ;
```

```nudl
fn process<T, U>(input: T, transform: (T) -> U) -> U
where
    T: Clone + Printable,
    U: Printable,
{
    let copy = input.clone();
    println(f"Processing: {copy.to_string()}");
    transform(input)
}
```

#### 3.5.3 Monomorphization

Generics are resolved by monomorphization at the **bytecode generation** stage.
The compiler first type-checks generic definitions once against their declared
bounds (ensuring that the generic body only uses operations available through
the bound interfaces). Then, for each unique combination of type arguments at a
call site or construction site, the compiler generates a **specialized SSA
bytecode** copy with all type parameters replaced by concrete types.

If a generic function is called as `max::<i32>(a, b)` and `max::<f64>(c, d)`,
two specialized SSA functions are generated: one operating on `i32` and one on
`f64`. The generic definition is type-checked once (against the `Ord` bound),
but specialized twice at the bytecode level.

#### 3.5.4 Turbofish Syntax

When type inference cannot determine the type arguments to a generic function,
the caller may provide them explicitly using the turbofish syntax:

```
turbofish  = '::' '<' type % ',' '>' ;
```

```nudl
let x = parse::<i64>("42");
let empty = Vec::new::<string>();
```

#### 3.5.5 Type Variance

All generic type parameters are **invariant**. If `Cat` implements `Animal`,
`Cat[]` is *not* a subtype of `Animal[]`, and cannot be passed where `Animal[]`
is expected. This prevents the classic covariance bug where inserting a `Dog`
into what is actually a `Cat[]` causes a type error at runtime.

For polymorphic collections, use `dyn Interface`:

```nudl
let animals: (dyn Animal)[] = [cat, dog];  // explicit dynamic dispatch
```

### 3.6 Interfaces

#### 3.6.1 Interface Declarations

An interface defines a set of method signatures that types may implement.

```
interface_def   = 'interface' identifier generics? '{' interface_item* '}' ;
interface_item  = fn_signature ';'                /* required method */
                | fn_def ;                         /* default method  */
```

```nudl
interface Printable {
    fn to_string(self) -> string;
}

interface Summary {
    fn summarize(self) -> string;
    fn preview(self) -> string {
        let s = self.summarize();
        if s.len() > 50 {
            f"{s.substring(0, 50)}..."
        } else {
            s
        }
    }
}
```

Default methods have a body and need not be overridden by implementors.

#### 3.6.2 Generic Interfaces

Interfaces may have type parameters. nudl does not support associated types;
use generic parameters instead.

```nudl
interface Iterator<T> {
    fn next(mut self) -> Option<T>;
}

interface Index<Idx, Output> {
    fn index(self, idx: Idx) -> Output;
}

interface Add<Rhs, Output> {
    fn add(self, rhs: Rhs) -> Output;
}
```

#### 3.6.3 Interface Implementation

```
impl_interface  = 'impl' generics? path 'for' type where_clause? '{' fn_def* '}' ;
```

```nudl
impl Printable for Point {
    fn to_string(self) -> string {
        f"({self.x}, {self.y})"
    }
}

impl<T: Printable> Printable for Option<T> {
    fn to_string(self) -> string {
        match self {
            Some(v) => f"Some({v.to_string()})",
            None => "None",
        }
    }
}
```

An implementation must provide all required methods (those without default
bodies). It may override default methods.

#### 3.6.4 Method Resolution

When a method is called on a value, the compiler resolves the method as follows:

1. **Inherent methods** (defined in `impl Type { ... }`) are searched first.
2. **Interface methods** (from all interfaces implemented by the type) are
   searched second.
3. If exactly one method matches, it is selected.
4. If multiple interface methods match and no inherent method exists, the call
   is ambiguous and the compiler reports an error.

**Qualified disambiguation:** When ambiguity exists, the caller must use
qualified syntax to specify which interface's method is intended:

```nudl
interface A { fn name(self) -> string; }
interface B { fn name(self) -> string; }

impl A for Foo { fn name(self) -> string { "A" } }
impl B for Foo { fn name(self) -> string { "B" } }

let foo = Foo {};
// foo.name();             // ERROR: ambiguous
let a_name = A::name(foo);   // "A"
let b_name = B::name(foo);   // "B"
```

#### 3.6.5 Blanket Implementations

Blanket implementations (implementing an interface for all types satisfying a
bound) are not supported in version 1 of nudl. Each implementation must name a
concrete type or a specific generic instantiation.

### 3.7 Dynamic Dispatch

#### 3.7.1 The dyn Type

The `dyn Interface` type enables runtime polymorphism. A value of type
`dyn Interface` can hold any value whose type implements `Interface`.

```
dyn_type  = 'dyn' path ;
```

`dyn Interface` is a reference type. Its runtime representation is a **fat
pointer** consisting of two machine words (16 bytes on 64-bit platforms):

- A pointer to the ARC-managed data (same as a regular reference).
- A pointer to a vtable containing the interface's method implementations for
  the concrete type.

**Vtable layout:** The vtable is a static, read-only table generated at compile
time for each (concrete type, interface) pair:

```
+-------------------+
| method_0: fn ptr  |   first interface method (declaration order)
| method_1: fn ptr  |   second interface method
| ...               |
| method_N: fn ptr  |   last interface method
+-------------------+
| drop_fn: fn ptr   |   destructor for the concrete type
+-------------------+
| size: u64         |   size of the concrete type in bytes
+-------------------+
```

Method function pointers are stored in the order the methods are declared in
the interface definition. The `drop_fn` is invoked when the `dyn` value's
reference count reaches zero. The `size` field enables the runtime to determine
the allocation size for deallocation.

```nudl
fn print_all(items: (dyn Printable)[]) {
    for item in items {
        println(item.to_string());
    }
}

let items: (dyn Printable)[] = [
    Point { x: 1.0, y: 2.0 },
    Color(255, 0, 0),
];
print_all(items);
```

#### 3.7.2 Restrictions

A `dyn Interface` type cannot be formed for interfaces that have methods with
generic type parameters, because the vtable cannot accommodate an unbounded
number of monomorphized method entries.

```nudl
interface Container<T> {
    fn get(self, index: i32) -> T;           // OK: T is interface-level
}

interface Transformer {
    fn transform<U>(self, value: U) -> U;    // Cannot use with dyn
}

let c: dyn Container<i32> = /* ... */;       // OK
// let t: dyn Transformer = /* ... */;       // ERROR
```

---

## 4. Memory Model

### 4.1 Value Types vs Reference Types

Every type in nudl is classified as either a value type or a reference type.
This classification determines assignment semantics, storage location, and
lifetime management.

**Value types** are stored inline (on the stack or within their containing
object). Assignment copies the value.

| Category                               | Examples                      |
|----------------------------------------|-------------------------------|
| Integer types                          | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` |
| Floating-point types                   | `f32`, `f64`                  |
| Boolean                                | `bool`                        |
| Character                              | `char`                        |
| Unit                                   | `()`                          |
| FFI pointer types                      | `RawPtr`, `MutRawPtr`, `CStr` |
| Tuples of value types                  | `(i32, f64)`, `(bool, char)`  |
| Fixed-size arrays of value types       | `[i32; 4]`, `[f64; 3]`       |

**Reference types** are heap-allocated and managed by automatic reference
counting. Assignment increments the reference count; both bindings alias the
same allocation.

| Category                               | Examples                           |
|----------------------------------------|------------------------------------|
| Structs                                | `Point`, `Config`                  |
| Enums                                  | `Option<T>`, `Result<T, E>`        |
| Strings                                | `string`                           |
| Dynamic arrays                         | `i32[]`, `Point[]`                 |
| Maps                                   | `Map<string, i32>`                 |
| Closures                               | `(i32) -> i32`                     |
| Dynamic dispatch objects               | `dyn Printable`                    |
| Futures                                | `Future<i32>`, `Future<string>`    |
| Range types                            | `Range<T>`, `RangeInclusive<T>`    |
| Hash set                               | `Set<T>` (T must implement `Eq`)   |
| Actors                                 | `Counter`, `ChatRoom`              |

### 4.2 Automatic Reference Counting

Reference types are managed by ARC. The compiler inserts retain (increment
reference count) and release (decrement reference count) operations at the
following points:

1. **Allocation.** A new heap object is created with a reference count of 1.
2. **Assignment and binding.** When a reference-type value is bound to a new
   name or stored into a field, the reference count is incremented. If the
   destination previously held a different reference, the old reference's count
   is decremented first.
3. **Function arguments.** The calling convention for reference-type arguments
   is **caller-retain / callee-release**: the caller emits a retain before the
   call, and the callee is responsible for releasing each reference-type
   parameter at function exit (or when the parameter binding goes out of scope).
4. **Scope exit.** When a binding goes out of scope, its reference count is
   decremented. If the count reaches zero, the object is deallocated.

The reference count is a non-atomic 32-bit unsigned integer in version 1 (nudl
v1 is single-threaded).

**Object layout in memory:**

```
+-------------------+  offset 0
| strong_count: u32 |
+-------------------+  offset 4
| weak_count:   u32 |
+-------------------+  offset 8
| type_tag:     u32 |
+-------------------+  offset 12
| padding:      u32 |
+-------------------+  offset 16
| field_0           |
| field_1           |
| ...               |
+-------------------+
```

The header occupies 16 bytes, followed by the object's fields laid out
according to the type's definition with platform-appropriate alignment.

### 4.3 Assignment Semantics

**Value type assignment** produces an independent copy:

```nudl
let a: i32 = 42;
let mut b = a;     // b is a copy of a
b = 99;
// a is still 42
```

**Reference type assignment** shares the allocation:

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a;         // refcount incremented; a and b alias same data
b.x = 99.0;
println(f"{a.x}");    // prints 99.0 (a and b share the same Point)
```

### 4.3a Aliased Mutation

Because reference types share their allocation through ARC, assigning a
reference-type value to a new binding does **not** create a copy. Both bindings
point to the same object, and mutations through either binding are visible
through the other. This is a fundamental property of nudl's memory model and
applies to all reference types: structs, enums, strings (immutable content, but
the binding can be reassigned), dynamic arrays, maps, closures, and `dyn`
values.

**This will surprise you if you expect value semantics.** The following example
is the single most important thing to understand about nudl's memory model:

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a;       // b and a are the SAME object (refcount = 2)
b.x = 99.0;
println(f"{a.x}");   // prints 99.0 — a sees b's mutation
```

This is **not** a bug. It is identical to how Swift classes, Java objects, and
Python objects behave. It is different from Rust (which would move `a` or
require an explicit reference), C++ (which would copy the struct), and Swift
value types (which use copy-on-write).

**When aliased mutation is useful:**

```nudl
// Shared configuration — changes propagate automatically
let config = AppConfig { debug: false, verbose: false };
let logger = Logger { config };  // logger.config aliases the same object
config.debug = true;             // logger sees the change immediately

// Shared mutable collections
let mut cache: Map<string, string> = Map::new();
let handler = RequestHandler { cache };
// handler and the outer scope share the same cache
```

**When aliased mutation is surprising:**

```nudl
fn make_modified(point: Point) -> Point {
    // DANGER: this modifies the caller's point too!
    point.x = 0.0;
    point
}

let mut p = Point { x: 5.0, y: 10.0 };
let q = make_modified(p);
// p.x is now 0.0 — probably not what the caller expected
```

**Use `.clone()` to get an independent copy:**

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a.clone();   // independent allocation, refcount = 1
b.x = 99.0;
println(f"{a.x}");       // prints 1.0 — a is unaffected

fn make_modified(point: Point) -> Point {
    let mut copy = point.clone();
    copy.x = 0.0;
    copy
}
```

**Rules of thumb:**

1. If a function receives a reference type and should not affect the caller's
   value, clone it before mutating.
2. If you want two bindings to evolve independently, clone at the point of
   divergence.
3. If you want shared state (caches, configuration, observable state), aliasing
   is the right tool — no extra work needed.
4. When storing a value in a struct field, remember that the field and the
   original binding alias the same object.

### 4.4 Weak References

Weak references allow referencing an object without preventing its deallocation.
They are used to break reference cycles.

```
weak_let  = 'let' 'weak' identifier '=' expression ';' ;
```

A weak reference has type `WeakRef<T>` internally, but `WeakRef<T>` is never
written in user source code — it exists only as a compiler-internal type
introduced by the `weak` modifier. The only way to create a weak reference is
through the `let weak` binding syntax or the `weak` field modifier. To access
the referent, the weak reference must be upgraded:

```nudl
let strong = Node { value: 42, children: [] };
let weak r = strong;

match r.upgrade() {
    Some(node) => println(f"Value: {node.value}"),
    None => println("Object has been deallocated"),
}
```

**Upgrade semantics:** `r.upgrade()` returns `Option<T>`. If the referent's
strong count is greater than zero, it increments the strong count and returns
`Some(strong_reference)`. If the strong count is zero, it returns `None`.

**Deallocation with weak references:** When an object's strong count reaches
zero, its fields are destroyed (drop is called, field reference counts are
decremented). However, the object's header is kept alive until the weak count
also reaches zero, so that weak references can observe the zero strong count
and return `None` from `upgrade`.

**Weak references in struct fields:**

```nudl
struct Node {
    value: i32,
    parent: Option<weak Node>,     // weak reference breaks cycle
    children: Node[],
}
```

### 4.5 Compile-Time Cycle Detection

The compiler builds a field reference graph where nodes are types and directed
edges represent reference-type fields. If a cycle is detected in this graph,
the compiler emits a warning suggesting the use of `weak` on one of the
back-edges:

```
warning[W0301]: potential reference cycle detected
  --> src/main.nudl:5:5
   |
5  |     parent: Option<Node>,
   |     ^^^^^^
   |
   = note: Node.children -> Node[] -> Node.parent -> Node
   = help: consider using `weak` for the `parent` field:
           parent: Option<weak Node>,
```

This analysis is conservative: it warns about structural cycles in type
definitions, not about whether cycles actually form at runtime.

### 4.6 Mutability

Mutability in nudl is a property of bindings, not types.

- `let x = expr;` creates an immutable binding. The value of `x` cannot be
  changed, and fields of `x` cannot be assigned through `x`.
- `let mut x = expr;` creates a mutable binding. The value can be reassigned,
  and fields can be mutated through `x`.

**Aliased mutation:** Because reference types share their allocation through
ARC, mutating a value through one mutable binding is visible through all other
bindings to the same allocation:

```nudl
let mut a = Point { x: 1.0, y: 2.0 };
let mut b = a;       // shared alias
b.x = 5.0;
// a.x is now 5.0
```

This is the same model as Swift and Java. nudl does not have a borrow checker
and does not prevent aliased mutation.

**Method receivers:** Methods declare their receiver as `self` (immutable) or
`mut self` (mutable). A method with `mut self` may modify the receiver's
fields.

```nudl
impl Counter {
    fn get(self) -> i32 { self.value }
    fn increment(mut self) { self.value = self.value + 1; }
}

let counter = Counter { value: 0 };
// counter.increment();  // ERROR: counter is not mut
let mut counter = Counter { value: 0 };
counter.increment();     // OK
```

### 4.7 Clone

The `Clone` interface provides deep-copy semantics for reference types:

```nudl
interface Clone {
    fn clone(self) -> Self;
}
```

Calling `.clone()` on a reference type allocates a new object and recursively
copies all fields. For value types, `.clone()` is equivalent to copying the
value.

```nudl
let a = Point { x: 1.0, y: 2.0 };
let mut b = a.clone();    // independent allocation
b.x = 99.0;
// a.x is still 1.0
```

The compiler can auto-derive `Clone` for structs and enums whose fields all
implement `Clone`.

### 4.8 Drop

The `Drop` interface provides custom cleanup logic:

```nudl
interface Drop {
    fn drop(mut self);
}
```

The `drop` method is called automatically when:

- A reference type's strong count reaches zero.
- A value type with a `Drop` implementation goes out of scope.

**Binding drop order:** Within a scope, bindings are dropped in **reverse
declaration order** (LIFO — last declared is dropped first). `defer` blocks
execute before drop calls at scope exit.

**Struct field drop order:** When a struct's reference count reaches zero and
its `drop` method (if any) has executed, the struct's fields are released in
**declaration order** (first field declared is dropped first).

**Enum drop order:** When an enum value is dropped, the fields of the **active
variant** are dropped in declaration order, consistent with struct field drop
behavior. Inactive variants are not dropped (they hold no data).

```nudl
struct FileHandle { fd: i32 }

impl Drop for FileHandle {
    fn drop(mut self) {
        close_fd(self.fd);
    }
}

fn process() {
    let a = FileHandle { fd: open("a.txt") };  // dropped second (reverse decl order)
    let b = FileHandle { fd: open("b.txt") };  // dropped first
    // On scope exit: b.drop() called, then a.drop()
}

struct Connection {
    socket: Socket,      // dropped first (declaration order)
    buffer: Buffer,      // dropped second
    logger: Logger,      // dropped third
}
```

### 4.9 ARC Counter Overflow

If the strong or weak reference count reaches `u32::MAX`, the program
immediately aborts with a diagnostic message: "ARC reference count overflow".
No unwinding occurs — defer blocks and Drop implementations are not executed.
This matches the behavior of a hardware fault and prevents silent wraparound.

> **v2 Note:** In v1, all ARC operations are non-atomic (single-threaded). In
> v2, when multi-threading support is added, all ARC operations will switch to
> atomic operations. This is an ABI-level change — v1 and v2 compiled code are
> not binary-compatible. Source code requires no changes.

---

