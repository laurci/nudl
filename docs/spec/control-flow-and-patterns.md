## 7. Statements

### 7.1 Let Bindings

```
let_stmt  = 'let' 'weak'? 'mut'? pattern ( ':' type )? '=' expression ';' ;
```

A `let` statement introduces one or more bindings by matching a pattern against
the initializer expression.

```nudl
let x = 42;                               // immutable binding
let mut y = 0;                             // mutable binding
let z: f64 = 3.14;                         // explicit type annotation
let (a, b) = (10, "hello");                // destructuring
let Point { x, y } = point;               // struct destructuring
let weak parent_ref = parent;             // weak reference
```

The initializer is required. There are no uninitialized bindings.

### 7.2 Expression Statements

```
expr_stmt  = expression ';' ;
```

An expression followed by a semicolon is evaluated for its side effects and the
result is discarded.

```nudl
println("hello");          // return value () discarded
compute_value();           // return value discarded
x + y;                     // computed but discarded (compiler may warn)
```

### 7.3 Defer

```
defer_stmt  = 'defer' block_expr ;
```

A `defer` statement schedules a block to execute when the enclosing scope exits,
regardless of whether the scope exits normally, via `return`, `break`,
`continue`, or `?` propagation.

Multiple defers in the same scope execute in LIFO (last-in, first-out) order.

```nudl
fn process_file(path: string) -> Result<string, Error> {
    let file = open(path)?;
    defer { file.close(); }

    let lock = file.lock()?;
    defer { lock.release(); }

    let content = file.read_all()?;
    // On scope exit (normal or early return):
    //   1. lock.release()
    //   2. file.close()
    Ok(content)
}
```

Defer blocks execute before `Drop` calls at scope exit.

Defer blocks execute on **any** scope exit, including early returns triggered by
the `?` operator:

```nudl
fn read_config(path: string) -> Result<Config, Error> {
    let file = open(path)?;       // if this fails, defer below doesn't run (not yet registered)
    defer { file.close(); }       // registered here
    let data = file.read_all()?;  // if this fails, defer DOES run before returning Err
    parse(data)?                  // defer runs here too on error
}
```

The `?` operator desugars to an early `return Err(...)`. Since `return` is a
scope exit, all defer blocks registered before the `?` execute in LIFO order
before the function returns. This makes defer reliable for resource cleanup in
error-prone code.

### 7.4 Item Statements

Functions, structs, enums, and other items may be declared inside block
expressions. They are visible only within the enclosing block.

```nudl
fn outer() {
    struct LocalPoint { x: f64, y: f64 }

    fn local_helper(p: LocalPoint) -> f64 {
        p.x + p.y
    }

    let p = LocalPoint { x: 1.0, y: 2.0 };
    println(f"{local_helper(p)}");
}
```

---

## 8. Pattern Matching

### 8.1 Match Expressions

```
match_expr  = 'match' expression '{' match_arm % ',' '}' ;
match_arm   = pattern ( 'if' expression )? '=>' expression ;
```

The scrutinee is evaluated once. Arms are tested in order. The first arm whose
pattern matches (and whose guard, if present, evaluates to `true`) is selected.
The arm's expression is evaluated and its value becomes the value of the match
expression.

### 8.2 Pattern Types

```
pattern  = literal_pattern
         | binding_pattern
         | wildcard_pattern
         | tuple_pattern
         | struct_pattern
         | enum_pattern
         | or_pattern
         | range_pattern
         | '(' pattern ')' ;

literal_pattern   = integer_literal | float_literal | char_literal
                  | string_literal | bool_literal ;
binding_pattern   = 'mut'? identifier ;
wildcard_pattern  = '_' ;
tuple_pattern     = '(' pattern % ',' ')' ;
struct_pattern    = path '{' field_pattern % ',' ( ',' '..' )? '}' ;
field_pattern     = identifier ':' pattern | identifier ;
enum_pattern      = path ( '(' pattern % ',' ')' )?
                  | path '{' field_pattern % ',' '}' ;
or_pattern        = pattern ( '|' pattern )+ ;
range_pattern     = expression '..' expression
                  | expression '..=' expression ;
```

#### 8.2.1 Literal Patterns

Match against a specific literal value:

```nudl
match x {
    0 => "zero",
    1 => "one",
    42 => "answer",
    _ => "other",
}
```

#### 8.2.2 Binding Patterns

Bind the matched value to a new variable:

```nudl
match x {
    n => println(f"got {n}"),
}
```

Bindings introduced in patterns are available in the arm's guard and expression.

#### 8.2.3 Wildcard Patterns

The wildcard `_` matches any value without binding it:

```nudl
match pair {
    (_, 0) => "second is zero",
    (0, _) => "first is zero",
    _ => "neither is zero",
}
```

#### 8.2.4 Tuple Patterns

Destructure a tuple by its elements:

```nudl
match point {
    (0, 0) => "origin",
    (x, 0) => f"on x-axis at {x}",
    (0, y) => f"on y-axis at {y}",
    (x, y) => f"({x}, {y})",
}
```

#### 8.2.5 Struct Patterns

Destructure a struct by its fields. `..` ignores remaining fields:

```nudl
match config {
    Config { tls: true, port: 443, .. } => "standard HTTPS",
    Config { tls: true, port, .. } => f"HTTPS on port {port}",
    Config { host, .. } => f"plaintext to {host}",
}
```

#### 8.2.6 Enum Patterns

Match enum variants and destructure their payloads:

```nudl
match result {
    Ok(value) => println(f"Success: {value}"),
    Err(msg) => println(f"Error: {msg}"),
}

match shape {
    Shape::Circle(r) if r > 10.0 => "large circle",
    Shape::Circle(r) => f"small circle with radius {r}",
    Shape::Rectangle { width, height } => f"{width}x{height}",
    Shape::Point => "point",
}
```

#### 8.2.7 Or Patterns

Match any of several alternatives:

```nudl
match code {
    200 | 201 | 202 => "success",
    301 | 302 => "redirect",
    404 => "not found",
    500 | 502 | 503 => "server error",
    _ => "unknown",
}
```

All alternatives in an or-pattern must bind the same set of variables with the
same types.

#### 8.2.8 Range Patterns

Match a range of values:

```nudl
match ch {
    'a'..='z' => "lowercase",
    'A'..='Z' => "uppercase",
    '0'..='9' => "digit",
    _ => "other",
}
```

Range patterns are supported for integer types and `char`.

#### 8.2.9 Nested Patterns

Patterns may be nested to arbitrary depth:

```nudl
match expr {
    Some((x, Some(y))) => f"both present: {x}, {y}",
    Some((x, None)) => f"only first: {x}",
    None => "nothing",
}
```

#### 8.2.10 Reference Transparency

Patterns work through ARC references transparently. When matching a
reference-type value, the pattern destructures the underlying data without
explicit dereferencing.

### 8.3 Guards

A guard is a boolean expression that provides additional conditions beyond
structural matching:

```nudl
match value {
    n if n > 0 && n < 100 => "in range",
    n if n == 0 => "zero",
    n => "out of range",
}
```

Guards are evaluated after the pattern matches. Variables bound by the pattern
are available in the guard expression.

A match arm with a guard is not considered to cover its pattern exhaustively,
because the guard might be false.

### 8.4 Exhaustiveness Checking

The compiler verifies that a match expression covers all possible values of
the scrutinee type:

- **Enums:** All variants must be matched (directly or via `_`).
- **Bool:** Both `true` and `false` must be covered.
- **Integer types:** Requires `_` or complete coverage (impractical for most
  types, so `_` is typically required).
- **Structs/tuples:** A single binding or wildcard pattern suffices.
- **Nested types:** Exhaustiveness is checked recursively.

If the match is not exhaustive, the compiler reports an error listing the
uncovered patterns:

```
error[E0401]: non-exhaustive match
  --> src/main.nudl:15:5
   |
15 |     match opt {
   |     ^^^^^
   |
   = note: missing patterns: `None`
```

### 8.5 If-Let and While-Let

`if let` and `while let` are convenience forms for single-pattern matching:

```
if_let_expr     = 'if' 'let' pattern '=' expression block_expr
                  ( 'else' block_expr )? ;
while_let_expr  = 'while' 'let' pattern '=' expression block_expr ;
```

```nudl
// if let
if let Some(value) = map.get("key") {
    println(f"Found: {value}");
}

// while let
while let Some(item) = iterator.next() {
    process(item);
}
```

`if let` does not require exhaustiveness. The `else` branch handles all
non-matching values. `while let` loops until the pattern fails to match.

---

## 9. Error Handling

### 9.1 The Result Type

`Result<T, E>` is a built-in enum representing a computation that may fail:

```nudl
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

`Ok(value)` represents success, carrying the result value. `Err(error)`
represents failure, carrying the error value.

The error type `E` in `Result<T, E>` must implement the `Error` interface (see
Section 15.6). This ensures all error values provide a `.message()` method for
uniform error reporting.

```nudl
fn divide(a: f64, b: f64) -> Result<f64, string> {
    if b == 0.0 {
        Err("division by zero")
    } else {
        Ok(a / b)
    }
}
```

### 9.2 The Option Type

`Option<T>` is a built-in enum representing a value that may be absent:

```nudl
enum Option<T> {
    Some(T),
    None,
}
```

`Some(value)` contains a value. `None` represents absence. There is no null in
nudl.

```nudl
fn find_first_even(numbers: i32[]) -> Option<i32> {
    for n in numbers {
        if n % 2 == 0 {
            return Some(n);
        }
    }
    None
}
```

### 9.3 The ? Operator

The `?` operator provides concise error propagation. See Section 6.14 for
syntax and semantics.

**Type compatibility and automatic conversion:** The error type in the
expression must be convertible to the error type of the enclosing function's
`Result`. If the types differ, the compiler automatically inserts a
`From::from()` call to convert the error (see Section 15.7). If no `From`
implementation exists for the conversion, the compiler reports an error.

The `?` operator applies **one direct `From` conversion** only — no transitive
chaining. If no direct `From<SourceError, TargetError>` implementation exists,
it is a compile error. For example, if `From<A, B>` and `From<B, C>` are both
implemented but `From<A, C>` is not, using `?` on a `Result<T, A>` inside a
function returning `Result<U, C>` will fail to compile.

```nudl
// With From<IoError, AppError> and From<ParseError, AppError> implemented:
fn read_config(path: string) -> Result<Config, AppError> {
    let content = read_file(path)?;      // IoError auto-converted to AppError
    let config = parse_toml(content)?;    // ParseError auto-converted to AppError
    Ok(config)
}

// Manual conversion is still available when From is not implemented:
fn fallback(path: string) -> Result<Config, AppError> {
    let content = read_file(path).map_err(AppError::from_io)?;
    Ok(parse(content))
}
```

### 9.4 Optional Parameters

Optional parameters use `?` in the parameter declaration (distinct from the `?`
error propagation operator on expressions):

```nudl
fn connect(host: string, port?: u16, timeout?: u64) -> Connection {
    let actual_port = match port {
        Some(p) => p,
        None => 80,
    };
    let actual_timeout = match timeout {
        Some(t) => t,
        None => 30000,
    };
    // ...
}

connect("example.com");
connect("example.com", port: 443);
connect("example.com", port: 443, timeout: 5000);
```

An optional parameter `name?: Type` has type `Option<Type>` within the
function body. If omitted by the caller, it defaults to `None`.

### 9.5 Panic

```nudl
panic("unrecoverable error");
```

`panic` terminates the program immediately with an error message. In version 1,
there is no unwinding: the process exits immediately after printing the panic
message and a stack trace (if available).

`panic` has return type `!` (the never type; see Section 3.1.8), meaning it
never returns and can be used in any expression context.

---

## 10. Control Flow

### 10.1 If / Else

See Section 6.10 for the grammar and expression semantics.

As a statement, `if` does not require an `else` branch:

```nudl
if should_log {
    println(f"Value: {x}");
}
```

As an expression, `else` is required and both branches must yield the same type:

```nudl
let sign = if x > 0 { 1 } else if x < 0 { -1 } else { 0 };
```

### 10.2 For Loops

```
for_expr  = 'for' pattern 'in' expression block_expr ;
```

The iterable expression must implement the `Iterator<T>` interface (or be a
type for which an `into_iter()` method exists that returns an `Iterator<T>`).

```nudl
for item in collection {
    process(item);
}

for (index, value) in collection.enumerate() {
    println(f"[{index}] = {value}");
}

for i in 0..10 {
    println(f"{i}");
}

for ch in "hello".chars() {
    println(f"char: {ch}");
}
```

The value of a `for` expression is `()`.

### 10.3 While Loops

```
while_expr  = 'while' expression block_expr ;
```

The condition must have type `bool`. The loop executes its body as long as the
condition evaluates to `true`.

```nudl
let mut count = 0;
while count < 10 {
    count += 1;
}
```

The value of a `while` expression is `()`.

### 10.4 Loop

```
loop_expr  = 'loop' block_expr ;
```

`loop` creates an infinite loop. The loop can only be exited via `break`,
`return`, or `panic`.

```nudl
let result = loop {
    let input = read_line();
    if let Ok(number) = parse_i32(input) {
        break number;         // loop evaluates to number
    }
    println("Invalid input, try again.");
};
```

When `break` is used with a value, the `loop` expression evaluates to that
value. All `break` expressions within a loop must yield the same type.

### 10.5 Break and Continue

```
break_expr     = 'break' label? expression? ;
continue_expr  = 'continue' label? ;
label          = '\'' identifier ;
```

`break` exits the innermost enclosing loop (or the loop identified by the
label). `continue` skips to the next iteration.

```nudl
for i in 0..100 {
    if i % 2 == 0 { continue; }
    if i > 50 { break; }
    println(f"{i}");
}
```

### 10.6 Labeled Loops

Loops may be labeled to allow `break` and `continue` to target outer loops:

```nudl
'outer: for i in 0..10 {
    for j in 0..10 {
        if i + j > 15 {
            break 'outer;
        }
        if j % 2 == 0 {
            continue 'outer;
        }
    }
}
```

Labels are prefixed with `'` (single quote) followed by an identifier.

Labeled breaks may carry a value, just like unlabeled breaks:

```nudl
let found = 'search: for row in matrix {
    for cell in row {
        if cell.matches(target) {
            break 'search cell;
        }
    }
};
```

All `break` expressions targeting the same label must yield the same type. If a
for-in loop has both labeled break-with-value paths and a normal exhaustion
path, the loop expression type is `Option<T>` — `break 'label value` produces
`Some(value)`, and normal loop exhaustion produces `None`.

```nudl
let maybe = 'outer: for item in list {
    if item.is_special() {
        break 'outer item;     // Some(item)
    }
};
// maybe: Option<Item> — None if loop exhausted without break
```

---

