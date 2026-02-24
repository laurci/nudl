## 5. Items and Declarations

### 5.1 Functions

```
fn_def         = visibility? 'comptime'? 'async'? 'fn' identifier generics?
                 '(' fn_params? ')' ( '->' type )? where_clause?
                 block_expr ;

fn_params      = fn_param % ',' ;
fn_param       = 'self'
               | 'mut' 'self'
               | identifier '?'? ':' type ( '=' expression )? ;
```

The return type may be omitted, in which case it defaults to `()`.

The last expression in the function body, if it lacks a trailing semicolon, is
the function's return value. An explicit `return expr;` statement may be used
for early return.

```nudl
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn greet(name: string) {
    println(`Hello, {name}!`);
}

fn divide(a: f64, b: f64) -> Result<f64, string> {
    if b == 0.0 {
        return Err("division by zero");
    }
    Ok(a / b)
}
```

### 5.2 Methods and Impl Blocks

```
impl_block  = 'impl' generics? type where_clause? '{' fn_def* '}' ;
```

Methods are functions defined inside `impl` blocks. The first parameter of a
method may be `self` or `mut self`, which binds the receiver.

- `self`: immutable reference to the receiver.
- `mut self`: mutable reference to the receiver.
- No `self`: static method, called as `Type::method()`.

The `self` parameter is invisible to the calling convention: it is not counted
as the first positional parameter for the named-argument rule (see Section
11.2).

```nudl
struct Circle {
    radius: f64,
}

impl Circle {
    // Static method (constructor)
    fn new(radius: f64) -> Circle {
        Circle { radius }
    }

    // Immutable method
    fn area(self) -> f64 {
        3.14159265358979 * self.radius * self.radius
    }

    // Mutable method
    fn scale(mut self, factor: f64) {
        self.radius = self.radius * factor;
    }
}

let mut c = Circle::new(5.0);
let a = c.area();          // self = c (immutable access)
c.scale(2.0);              // mut self = c (mutable access)
```

Multiple `impl` blocks may exist for the same type, but they must be in the
same module as the type's definition.

### 5.3 Structs

See Section 3.3.1 for the grammar and semantics of struct definitions.

**Struct construction** uses field-name syntax. If a variable in scope has the
same name as a field, the shorthand `field` may be used instead of
`field: field`:

```nudl
let x = 1.0;
let y = 2.0;
let p = Point { x, y };       // shorthand for Point { x: x, y: y }
```

**Struct update (spread) syntax** creates a new struct with some fields
overridden:

```nudl
let base = Config { host: "localhost", port: 8080, tls: false };
let prod = Config { ...base, host: "prod.example.com", tls: true };
// prod.port is 8080 (from base)
```

The spread source must be the same struct type. Spread must appear first in the
field list. All non-spread fields override the spread source's values.

**Struct field defaults:**

Struct fields may have default values, specified with `= expr` after the type:

```nudl
struct Config {
    host: string = "localhost",
    port: u16 = 8080,
    tls: bool = false,
}
```

Fields with defaults can be omitted during construction:

```nudl
let cfg = Config { tls: true };  // host and port use defaults
```

Default value expressions are evaluated fresh at each construction site. They
follow the same rules as function default parameters — any expression is valid,
and each evaluation is independent.

Fields without defaults must always be provided. In a struct literal, required
fields must appear before or alongside defaulted fields — the compiler does not
enforce ordering, but all required fields must be present.

### 5.4 Enums

See Section 3.3.2 for the grammar and semantics of enum definitions.

### 5.5 Interfaces

See Section 3.6 for the full treatment of interface declarations, generic
interfaces, and implementation.

### 5.6 Type Aliases

See Section 3.3.3.

Type aliases are **transparent** — they are an alternative name for the same
type, not a distinct type. `type UserId = u64` means `UserId` and `u64` are
fully interchangeable in all contexts: function parameters, return types,
pattern matching, and interface implementations.

```nudl
type UserId = u64;
let id: UserId = 42;
let n: u64 = id;        // no conversion needed
fn process(x: u64) {}
process(id);             // works — same type
```

### 5.7 Constants

```
const_def  = visibility? 'const' identifier ':' type '=' expression ';' ;
```

A constant declaration introduces a named compile-time value. The initializer
must be evaluable at compile time (same restrictions as comptime blocks — see
Section 13). The type annotation is required.

```nudl
const MAX_RETRIES: u32 = 3;
const PI: f64 = 3.14159265358979;
const GREETING: string = "hello, nudl";
const ORIGIN: (f64, f64) = (0.0, 0.0);
```

**Allowed types:** Constants may have any type that can cross the comptime
boundary: primitive types (integers, floats, `bool`, `char`), `string`, tuples
of allowed types, and fixed-size arrays of allowed types. Reference types
(structs, enums, dynamic arrays, maps, closures) are not allowed because
constants are embedded directly into the binary — they have no ARC header and
no heap allocation.

**Semantics:** Constants are inlined at every use site. They behave as if the
initializer expression were written directly at the point of use. String
constants are placed in read-only data and share a single allocation (no ARC
overhead).

**Scope:** Constants may appear at module level or inside block scopes. They
follow the same visibility rules as other items (`pub const` for public
visibility).

```nudl
pub const VERSION: string = "0.1.0";

fn example() {
    const LOCAL_LIMIT: i32 = 100;
    for i in 0..LOCAL_LIMIT {
        // ...
    }
}
```

**Interaction with comptime:** Constants defined with `const` are available in
comptime blocks. Build script `add_define()` values are also accessible as
constants (see Section 13.10).

### 5.8 Visibility

```
visibility  = 'pub' ;
```

By default, all items are private to their defining module. The `pub` keyword
makes an item visible to importing modules.

Visibility applies to:

- Functions
- Structs (the type name itself)
- Struct fields (each field independently)
- Enums (the type name; all variants of a public enum are public)
- Interfaces
- Type aliases
- Methods in impl blocks

```nudl
pub struct Point {
    pub x: f64,       // accessible outside the module
    pub y: f64,
    label: string,    // private (default)
}

pub fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn helper() -> f64 { 0.0 }   // private
```

### 5.9 Actor Declarations

See Section 17.7 for the full grammar and semantics of actor definitions.
Actors follow the same visibility rules as structs: the actor name and each
field/method can be independently marked `pub`.

---

## 6. Expressions

Every expression in nudl evaluates to a value and has a type. nudl is
expression-based: most constructs that are statements in other languages
(if/else, match, blocks) are expressions in nudl.

### 6.1 Literals

```
literal_expr  = integer_literal
              | float_literal
              | string_literal
              | template_string
              | char_literal
              | bool_literal
              | array_literal
              | tuple_literal ;

array_literal  = '[' expression % ',' ']'
               | '[' expression ';' expression ']' ;

tuple_literal  = '(' expression ',' ( expression % ',' )? ')' ;
```

Array repeat syntax `[expr; count]` evaluates `expr` once and repeats it
`count` times:

```nudl
let zeros = [0; 10];           // [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
let grid = [[0; 3]; 3];       // [[0,0,0], [0,0,0], [0,0,0]]
```

### 6.2 Path Expressions

```
path_expr  = identifier ( '::' identifier )* turbofish? ;
```

Paths name items: local variables, functions, types, enum variants, and module
members.

```nudl
foo                        // local variable or function
std::collections::Map      // module path
Option::Some               // enum variant
parse::<i32>               // function with turbofish
```

### 6.3 Block Expressions

```
block_expr  = '{' statement* expression? '}' ;
```

A block is a sequence of statements optionally followed by a tail expression
(an expression without a trailing semicolon). The value of the block is:

- The value of the tail expression, if present.
- `()`, if the block ends with a semicolon or is empty.

```nudl
let x = {
    let a = 10;
    let b = 20;
    a + b               // tail expression: block value is 30
};

let y = {
    println("side effect");
    // no tail expression: block value is ()
};
```

### 6.4 Operators

#### 6.4.1 Operator Precedence

Operators are listed from highest to lowest precedence. Operators at the same
precedence level share the specified associativity.

| Prec | Operators                        | Assoc  | Description           |
|------|----------------------------------|--------|-----------------------|
| 15   | `.`, `[]`, `()`                  | Left   | Field, index, call    |
| 14   | `?`, `.await`                    | Postfix| Error prop, await     |
| 13   | `-` (unary), `!`                 | Prefix | Negation, logical NOT |
| 12   | `as`                             | Left   | Type cast             |
| 11   | `*`, `/`, `%`                    | Left   | Multiplicative        |
| 10   | `+`, `-`                         | Left   | Additive              |
| 9    | `<<`, `>>`                       | Left   | Shift                 |
| 8    | `&`                              | Left   | Bitwise AND           |
| 7    | `^`                              | Left   | Bitwise XOR           |
| 6    | `\|`                             | Left   | Bitwise OR            |
| 5    | `==`, `!=`, `<`, `>`, `<=`, `>=` | None   | Comparison            |
| 4    | `&&`                             | Left   | Logical AND           |
| 3    | `\|\|`                           | Left   | Logical OR            |
| 2    | `..`, `..=`                      | None   | Range                 |
| 1    | `\|>`                            | Left   | Pipe                  |
| 0    | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `\|=`, `^=`, `<<=`, `>>=` | Right | Assignment            |

Prefix `await` is not in this table — like `return`, it is a prefix keyword
expression, not an operator.

Comparison operators are **non-chaining**: `a < b < c` is a syntax error. Use
`a < b && b < c` instead.

#### 6.4.2 Arithmetic Operators

Binary arithmetic operators `+`, `-`, `*`, `/`, `%` operate on numeric types.
Both operands must have the same type; no implicit widening occurs.

Unary `-` negates a numeric value. It is valid on signed integers and
floating-point types.

```nudl
let sum = 1 + 2;          // 3: i32
let diff = 5.0 - 2.5;     // 2.5: f64
let prod = 3 * 4;          // 12: i32
let quot = 10.0 / 3.0;    // 3.333...: f64
let rem = 10 % 3;          // 1: i32
let neg = -42;              // -42: i32
```

Integer division truncates toward zero: `-7 / 2 == -3`. The remainder operator
is consistent: `-7 % 2 == -1`. This matches C, Rust, Java, and Swift semantics.

Integer division or remainder by zero causes a **runtime panic**. The compiler
may report a compile-time error when the divisor is a literal zero.

**Integer overflow behavior:** All integer arithmetic wraps on overflow using
two's complement semantics in all build modes. There is no distinction between
debug and release builds for overflow behavior. See Section 3.1.1 for details.

#### 6.4.3 Comparison Operators

`==`, `!=` test equality. `<`, `>`, `<=`, `>=` test ordering. Both operands
must have the same type. The result type is `bool`.

Equality operators require the `Eq` interface. Ordering operators require the
`Ord` interface.

#### 6.4.4 Logical Operators

`&&` (logical AND) and `||` (logical OR) are short-circuit operators. Both
operands must have type `bool`. The result type is `bool`.

`!` (logical NOT) takes a single `bool` operand and returns `bool`.

```nudl
let result = a > 0 && b > 0;
let either = x || y;
let negated = !flag;
```

#### 6.4.5 Bitwise Operators

`&`, `|`, `^`, `<<`, `>>` operate on integer types. Both operands of binary
bitwise operators must have the same type.

#### 6.4.6 Assignment Operators

`=` assigns a value to a mutable place (variable, field, or index). Compound
assignment operators (`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`,
`>>=`) are syntactic sugar: `a op= b` is equivalent to `a = a op b`, except
that `a` is evaluated only once. The bitwise compound assignments (`&=`, `|=`,
`^=`, `<<=`, `>>=`) follow the same desugaring semantics as the arithmetic
ones, corresponding to the bitwise operators `&`, `|`, `^`, `<<`, `>>`.

Assignment expressions have type `()`.

```nudl
let mut x = 10;
x += 5;          // x is now 15
x *= 2;          // x is now 30

let mut flags = 0xFF00u16;
flags &= 0x0F0F;   // bitwise AND assign
flags |= 0x0001;   // bitwise OR assign
flags ^= 0x00FF;   // bitwise XOR assign
flags <<= 2;       // left shift assign
flags >>= 1;       // right shift assign
```

#### 6.4.7 Range Operators

`..` creates an exclusive range. `..=` creates an inclusive range. Both are used
in `for` loops and pattern matching.

```nudl
for i in 0..10 { /* 0, 1, 2, ..., 9 */ }
for i in 0..=10 { /* 0, 1, 2, ..., 10 */ }
```

### 6.5 Function Calls

```
call_expr  = expression '(' call_args? ')' trailing_lambda? ;

call_args  = call_arg % ',' ;
call_arg   = ( identifier ':' )? expression ;
```

See Section 11.2 for the full calling convention (positional first parameter,
named subsequent parameters, shorthand rules).

```nudl
let result = add(1, b: 2);
let greeting = greet("world");
let port = 8080;
send_request("https://example.com", method: "POST", port);
```

### 6.6 Field Access

```
field_expr  = expression '.' identifier
            | expression '.' integer_literal ;
```

Named fields are accessed by name. Tuple fields and tuple struct fields are
accessed by numeric index.

```nudl
let p = Point { x: 1.0, y: 2.0 };
let x_val = p.x;

let t = (10, "hello", true);
let first = t.0;        // 10
let second = t.1;       // "hello"
```

### 6.7 Index Access

```
index_expr  = expression '[' expression ']' ;
```

Index access uses the `Index<Idx, Output>` interface for reading and the
`IndexMut<Idx, Output>` interface for writing.

```nudl
let arr = [10, 20, 30];
let second = arr[1];       // 20

let mut map: Map<string, i32> = Map::new();
map["key"] = 42;
let value = map["key"];
```

Out-of-bounds array access is a runtime error (panic).

### 6.8 Closures

```
closure_expr  = '|' closure_params? '|' ( '->' type )? expression
              | '|' closure_params? '|' ( '->' type )? block_expr ;

closure_params  = closure_param % ',' ;
closure_param   = identifier ( ':' type )? ;
```

Closures are anonymous functions that capture variables from their enclosing
scope. All captures are by ARC reference: the closure increments the reference
count of each captured variable.

```nudl
let x = 10;
let add_x = |y: i32| -> i32 { x + y };
let result = add_x(5);    // 15

// Type annotations can often be omitted:
let double = |n| n * 2;

// Multi-line body:
let process = |items: i32[]| -> i32 {
    let mut sum = 0;
    for item in items {
        sum += item;
    }
    sum
};
```

Closures are reference types: they are heap-allocated and reference-counted.
Their type is `(ParamTypes) -> ReturnType`, which is the same as a function
type.

**Destructuring in parameters:** Closures support destructuring patterns in
their parameter lists, following the same pattern syntax used in `let`, `for`,
and `match`:

```nudl
// Tuple destructuring in closure
let pairs = [(1, "a"), (2, "b"), (3, "c")];
let firsts = map(pairs) { |(x, _)| x };

// Trailing lambda with destructuring
let sums = map(pairs) { (a, b) -> `{a}: {b}` };
```

Destructuring is permitted in all pattern positions:

- `let (a, b) = expr;` — let bindings
- `for (key, value) in map { ... }` — for loops
- `match expr { (x, y) => ... }` — match arms
- `|(x, y)| expr` and `{ (a, b) -> expr }` — closure and trailing lambda parameters

### 6.9 Trailing Lambda

When the last parameter of a function has a function type, the caller may pass
the argument as a trailing block after the closing parenthesis:

```
trailing_lambda  = '{' ( trailing_params '->' )? ( statement* expression? ) '}' ;

trailing_params  = identifier % ',' ;
```

**Single-parameter lambdas** receive an implicit parameter named `it`:

```nudl
let evens = filter(numbers) { it % 2 == 0 };
let doubled = map(numbers) { it * 2 };
```

**Multi-parameter lambdas** name their parameters before `->`:

```nudl
let sum = fold(numbers, initial: 0) { acc, item -> acc + item };
```

**Empty parentheses** may be omitted if there are no other arguments:

```nudl
fn run(action: () -> ()) { action(); }
run { println("hello"); }
```

Trailing lambdas use `{ }` exclusively. Pipe-syntax closures (`|params| expr`)
are for freestanding closures only. The two forms cannot be mixed.

### 6.10 If Expression

```
if_expr  = 'if' expression block_expr ( 'else' ( if_expr | block_expr ) )? ;
```

The condition must have type `bool`. When used as an expression (its value is
used), the `else` branch is required and both branches must have the same type.

```nudl
// As statement (no else required):
if x > 0 {
    println("positive");
}

// As expression (else required, types must match):
let abs_x = if x >= 0 { x } else { -x };

// Else-if chains:
let category = if score >= 90 {
    "excellent"
} else if score >= 70 {
    "good"
} else if score >= 50 {
    "passing"
} else {
    "failing"
};
```

**If-let:** Pattern matching can be used in if conditions:

```nudl
if let Some(value) = maybe_value {
    println(`Got: {value}`);
} else {
    println("Nothing");
}
```

### 6.11 Match Expression

```
match_expr  = 'match' expression '{' match_arm % ',' '}' ;

match_arm   = pattern ( 'if' expression )? '=>' expression ;
```

The scrutinee expression is matched against each arm's pattern in order. The
first matching arm's expression is evaluated and becomes the value of the match
expression.

Match must be **exhaustive**: the compiler verifies that all possible values of
the scrutinee type are covered. See Section 8 for pattern syntax and
exhaustiveness rules.

```nudl
let description = match shape {
    Shape::Circle(r) => `circle with radius {r}`,
    Shape::Rectangle { width, height } => `rectangle {width}x{height}`,
    Shape::Point => "point",
};

match (x, y) {
    (0, 0) => "origin",
    (x, 0) => `on x-axis at {x}`,
    (0, y) => `on y-axis at {y}`,
    (x, y) => `at ({x}, {y})`,
}
```

### 6.12 Struct and Array Construction

**Struct construction:**

```
struct_expr  = path '{' struct_field_init % ',' '}' ;

struct_field_init  = '...' expression
                   | identifier ':' expression
                   | identifier ;                 /* shorthand */
```

```nudl
let p = Point { x: 1.0, y: 2.0 };
let x = 1.0;
let y = 2.0;
let p = Point { x, y };                      // shorthand
let q = Point { ...p, x: 5.0 };              // spread
```

**Array construction:**

```
array_expr  = '[' ( '...' expression | expression ) % ',' ']'
            | '[' expression ';' expression ']' ;
```

```nudl
let arr = [1, 2, 3, 4, 5];
let repeated = [0; 100];                      // [0, 0, ..., 0] (100 elements)
let combined = [...arr, 6, 7, ...other];     // spread
```

### 6.13 Type Cast

```
cast_expr  = expression 'as' type ;
```

The `as` operator performs explicit type conversions between numeric types. No
implicit conversions exist in nudl.

Valid casts:

- Between any two integer types (truncation or zero/sign-extension).
- Between any two floating-point types.
- Between integer and floating-point types.

```nudl
let x: i32 = 42;
let y: i64 = x as i64;
let z: f64 = x as f64;
let w: u8 = 256i32 as u8;   // truncation: w == 0
```

### 6.14 Error Propagation

```
try_expr  = expression '?' ;
```

The `?` operator unwraps `Result<T, E>` or `Option<T>` values:

- On `Result<T, E>`: if the value is `Ok(v)`, evaluates to `v`. If `Err(e)`,
  returns `Err(e)` from the enclosing function immediately.
- On `Option<T>`: if the value is `Some(v)`, evaluates to `v`. If `None`,
  returns `None` from the enclosing function immediately.

The enclosing function must have a compatible return type (`Result<_, E>` or
`Option<_>`).

```nudl
fn parse_and_double(input: string) -> Result<i32, string> {
    let value = parse_i32(input)?;     // returns Err early if parse fails
    Ok(value * 2)
}

fn first_even(numbers: i32[]) -> Option<i32> {
    let first = numbers.first()?;      // returns None if empty
    if first % 2 == 0 { Some(first) } else { None }
}
```

### 6.15 Pipe Expressions

```
pipe_expr  = expression '|>' expression ;
```

The pipe operator `|>` is syntactic sugar for function application. It takes the
left-hand expression and passes it as the **first positional argument** to the
right-hand expression. The pipe operator has precedence 1 (between range and
assignment) with left associativity, enabling chained pipelines.

**Desugaring rules** (purely syntactic — the pipe desugars to a `Call` node
during parsing):

- `x |> f` desugars to `f(x)`
- `x |> f(y, z)` desugars to `f(x, y, z)` (prepend as first positional arg)
- `x |> obj.method` desugars to `obj.method(x)`
- `x |> obj.method(y)` desugars to `obj.method(x, y)`

**Interactions with other features:**

- **Named arguments stay named:** `data |> send(method: "POST")` desugars to
  `send(data, method: "POST")`.
- **Trailing lambdas:** `data |> filter { it > 0 }` desugars to
  `filter(data) { it > 0 }`.
- **With `.await`:** `fetch(url).await |> parse` desugars to
  `parse(fetch(url).await)` (`.await` binds at precedence 14, `|>` at 1).
- **No placeholder syntax.** The piped value always fills the first positional
  parameter. To pipe into a different position, use a closure:
  `x |> |v| f(other, second: v)`.

**Chaining example:**

```nudl
let result = data
    |> filter { it > 0 }
    |> map { it * 2 }
    |> fold(initial: 0) { acc, item -> acc + item };

// Equivalent to:
let result = fold(map(filter(data) { it > 0 }) { it * 2 }, initial: 0) { acc, item -> acc + item };
```

---

