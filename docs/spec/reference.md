## 14. Attributes

### 14.1 Syntax

```
attribute_list  = '#' '[' attribute ( ',' attribute )* ','? ']' ;
attribute       = identifier ( '=' literal )?
                | identifier ( '=' literal )? ( ',' identifier ( '=' literal )? )* ;
```

Attributes attach metadata to items (structs, enums, functions, fields,
variants). They are key-value pairs enclosed in `#[...]`.

```nudl
#[table = "users", version = 2]
struct User {
    #[column = "user_name", primary_key]
    name: string,

    #[column = "user_age"]
    age: u32,
}

#[deprecated = "use connect_tls instead"]
fn connect(host: string, port: u16 = 8080) -> Connection {
    // ...
}
```

Attribute values must be comptime literals: integers, floats, booleans, or
strings. An attribute key without a value is a boolean flag (equivalent to
`key = true`).

### 14.2 Multiple Attribute Lists

Multiple `#[...]` groups on the same item are merged:

```nudl
#[serializable]
#[table = "orders"]
struct Order { /* ... */ }
// Equivalent to: #[serializable, table = "orders"]
```

### 14.3 Reading Attributes at Comptime

Attributes are accessible via the `attributes()` and `field_attributes()`
comptime functions:

```nudl
comptime fn gen_insert(T: type) {
    let attrs = attributes(T);
    let table = attrs.get("table");   // "users"
    let fields = type_fields(T);

    let mut columns: string[] = [];
    let mut placeholders: string[] = [];

    for field in fields {
        let fattrs = field_attributes(T, field.name);
        let col = fattrs.get("column");
        columns.push(col);
        placeholders.push("?");
    }

    let cols = columns.join(", ");
    let vals = placeholders.join(", ");

    quote {
        impl ${T} {
            fn insert_sql() -> string {
                f"INSERT INTO ${table} (${cols}) VALUES (${vals})"
            }
        }
    }
}
```

### 14.4 Attribute Targets

Attributes may be placed on:

| Target | Example |
|---|---|
| Struct definitions | `#[serializable] struct Foo { ... }` |
| Enum definitions | `#[repr = "u8"] enum Color { ... }` |
| Struct fields | `struct S { #[skip] field: i32 }` |
| Enum variants | `enum E { #[default] A, B }` |
| Function definitions | `#[inline] fn foo() { ... }` |
| Interface definitions | `#[marker] interface Send {}` |

---

## 15. Built-in Interfaces

### 15.1 Clone

```nudl
interface Clone {
    fn clone(self) -> Self;
}
```

Produces a deep copy of the value. For reference types, `clone` allocates a new
object and recursively clones all fields. The compiler can auto-derive `Clone`
for types whose fields all implement `Clone`.

### 15.2 Drop

```nudl
interface Drop {
    fn drop(mut self);
}
```

Called automatically when:
- A reference type's strong count reaches zero.
- A value type with `Drop` goes out of scope.

The `drop` method must not be called manually by user code. It is invoked
exclusively by the runtime.

### 15.3 Printable

```nudl
interface Printable {
    fn to_string(self) -> string;
}
```

Used by `print`, `println`, and format string interpolation (`f"...{expr}..."`).
When an expression appears inside `{}` in a format string, its `to_string()`
method is called.

All primitive types implement `Printable`. User-defined types must implement it
explicitly or via comptime derivation.

### 15.4 Iterator

```nudl
interface Iterator<T> {
    fn next(mut self) -> Option<T>;
}
```

Used by `for` loops. The `for` loop repeatedly calls `next()` until it returns
`None`.

Iterator provides extension methods (defined as default methods or via blanket
patterns in the standard library):

```nudl
// Extension methods available on all Iterator<T>:
fn map<U>(self, transform: (T) -> U) -> MapIterator<T, U>;
fn filter(self, predicate: (T) -> bool) -> FilterIterator<T>;
fn fold<A>(self, initial: A, reducer: (A, T) -> A) -> A;
fn enumerate(self) -> EnumerateIterator<T>;
fn collect(self) -> T[];
fn any(self, predicate: (T) -> bool) -> bool;
fn all(self, predicate: (T) -> bool) -> bool;
fn count(self) -> u64;
fn take(self, n: u64) -> TakeIterator<T>;
fn skip(self, n: u64) -> SkipIterator<T>;
fn zip<U>(self, other: Iterator<U>) -> ZipIterator<T, U>;
```

Iterators are **fused**: once `next()` returns `None`, all subsequent calls to `next()` must also return `None`. This is a semantic requirement on all `Iterator<T>` implementations — consuming code may rely on this guarantee.

```nudl
let mut iter = [1, 2].iter();
assert(iter.next() == Some(1));
assert(iter.next() == Some(2));
assert(iter.next() == None);
assert(iter.next() == None);  // guaranteed — fused
```

### 15.5 Operator Overloading Interfaces

Operator overloading is achieved by implementing the corresponding interface.
Each operator maps to a specific interface method:

#### 15.5.1 Arithmetic Operators

```nudl
interface Add<Rhs, Output> {
    fn add(self, rhs: Rhs) -> Output;
}

interface Sub<Rhs, Output> {
    fn sub(self, rhs: Rhs) -> Output;
}

interface Mul<Rhs, Output> {
    fn mul(self, rhs: Rhs) -> Output;
}

interface Div<Rhs, Output> {
    fn div(self, rhs: Rhs) -> Output;
}

interface Rem<Rhs, Output> {
    fn rem(self, rhs: Rhs) -> Output;
}

interface Neg<Output> {
    fn neg(self) -> Output;
}
```

```nudl
impl Add<Vec2, Vec2> for Vec2 {
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2 { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

let a = Vec2 { x: 1.0, y: 2.0 };
let b = Vec2 { x: 3.0, y: 4.0 };
let c = a + b;    // calls Add::add(a, rhs: b)
```

#### 15.5.2 Logical NOT

```nudl
interface Not<Output> {
    fn not(self) -> Output;
}
```

#### 15.5.3 Equality

```nudl
interface Eq {
    fn eq(self, other: Self) -> bool;
}
```

Implementing `Eq` provides both `==` and `!=`. The `!=` operator is defined as
`!(self.eq(other))`.

#### 15.5.4 Ordering

```nudl
enum Ordering {
    Less,
    Equal,
    Greater,
}

interface Ord {
    fn cmp(self, other: Self) -> Ordering;
}
```

`Ord` requires `Eq`. Implementing `Ord` provides `<`, `>`, `<=`, `>=`. These
are defined in terms of `cmp`:

- `a < b` is `a.cmp(b) == Ordering::Less`
- `a > b` is `a.cmp(b) == Ordering::Greater`
- `a <= b` is `a.cmp(b) != Ordering::Greater`
- `a >= b` is `a.cmp(b) != Ordering::Less`

#### 15.5.5 Indexing

```nudl
interface Index<Idx, Output> {
    fn index(self, idx: Idx) -> Output;
}

interface IndexMut<Idx, Output> {
    fn index_mut(mut self, idx: Idx) -> Output;
}
```

`expr[idx]` in a reading context calls `Index::index`. In a writing context
(`expr[idx] = value`), it calls `IndexMut::index_mut`.

### 15.6 Error

```nudl
interface Error {
    fn message(self) -> string;
}
```

All error types used as the `E` parameter in `Result<T, E>` must implement the `Error` interface. This provides a uniform way to extract a human-readable error description from any error value.

Built-in error types (such as `IoError` and `ParseError`) implement `Error`. User-defined error types must provide an explicit implementation.

```nudl
struct AppError {
    code: i32,
    detail: string,
}

impl Error for AppError {
    fn message(self) -> string {
        f"AppError({self.code}): {self.detail}"
    }
}
```

### 15.7 From (Type Conversion)

```nudl
interface From<Source, Target> {
    fn from(source: Source) -> Target;
}
```

The `From` interface enables explicit type conversions. It is also used by the `?` operator for automatic error conversion: when the error type of the inner `Result` differs from the error type of the enclosing function's `Result`, the compiler inserts a `From::from()` call to convert between them.

```nudl
impl From<IoError, AppError> for AppError {
    fn from(source: IoError) -> AppError {
        AppError { code: 1, detail: source.message() }
    }
}

// Now ? auto-converts IoError -> AppError:
fn read_config(path: string) -> Result<Config, AppError> {
    let content = read_file(path)?;   // read_file returns Result<string, IoError>
    let config = parse(content)?;
    Ok(config)
}
```

---

## 16. Grammar Summary

This section provides an informal EBNF grammar covering all major productions
of the nudl language. This is a summary; the normative rules are in their
respective sections above.

### 16.1 Program Structure

```
program       = item* ;

item          = fn_def
              | struct_def
              | enum_def
              | interface_def
              | impl_block
              | impl_interface
              | type_alias
              | const_def
              | import_decl
              | comptime_block
              | actor_def
              | extern_block ;
```

### 16.2 Functions

```
fn_def        = visibility? 'comptime'? 'async'? 'fn' IDENT generics?
                '(' fn_params? ')' ( '->' type )? where_clause?
                block_expr ;

fn_params     = fn_param ( ',' fn_param )* ','? ;
fn_param      = 'mut'? 'self'
              | IDENT '?'? ':' type ( '=' expr )? ;

generics      = '<' generic_param ( ',' generic_param )* '>' ;
generic_param = IDENT ( ':' bounds )? ;
bounds        = path ( '+' path )* ;

where_clause  = 'where' where_pred ( ',' where_pred )* ;
where_pred    = IDENT ':' bounds ;
```

### 16.3 Types

The never type `!` is a built-in type representing diverging computations. It can be written as a function return type. `!` coerces to any type, allowing diverging expressions in any context. Functions returning `!` include `panic()` and user-defined functions that never return.

```
type          = path generics?                    /* named type          */
              | '(' type ( ',' type )* ','? ')'   /* tuple type          */
              | type '[]'                          /* dynamic array       */
              | '[' type ';' expr ']'              /* fixed-size array    */
              | 'Map' '<' type ',' type '>'        /* map                 */
              | 'Future' '<' type '>'              /* future type         */
              | '(' type ( ',' type )* ')' '->' type  /* function type   */
              | 'dyn' path                         /* dynamic dispatch    */
              | 'weak' type                        /* weak reference      */
              | '(' type ')'                       /* parenthesized       */
              | 'RawPtr'                            /* opaque const ptr    */
              | 'MutRawPtr'                         /* opaque mut ptr      */
              | 'CStr'                              /* C string ptr        */
              | 'Self'                              /* implementing type   */
              | '(' ')'                            /* unit                */ ;
```

### 16.4 Structs and Enums

```
struct_def    = visibility? 'struct' IDENT generics?
                ( '{' struct_field ( ',' struct_field )* ','? '}'
                | '(' type ( ',' type )* ')' ';'
                | ';' ) ;

struct_field  = visibility? IDENT ':' type ( '=' expression )? ;

enum_def      = visibility? 'enum' IDENT generics?
                '{' enum_variant ( ',' enum_variant )* ','? '}' ;

enum_variant  = IDENT
              | IDENT '(' type ( ',' type )* ')'
              | IDENT '{' struct_field ( ',' struct_field )* '}' ;
```

### 16.5 Interfaces and Implementations

```
interface_def = visibility? 'interface' IDENT generics?
                '{' interface_item* '}' ;

interface_item = fn_signature ';'
               | fn_def ;

fn_signature  = 'fn' IDENT generics? '(' fn_params? ')' ( '->' type )? ;

impl_block    = 'impl' generics? type where_clause? '{' fn_def* '}' ;

impl_interface = 'impl' generics? path 'for' type where_clause?
                 '{' fn_def* '}' ;
```

**Ambiguous method resolution:** When a type implements multiple interfaces that define a method with the same name, an unqualified call to that method is a compile error. The caller must use qualified syntax: `InterfaceName::method(receiver, args...)`. Inherent methods (in a plain `impl` block) always win over interface methods without ambiguity.

### 16.5a Constants

```
const_def     = visibility? 'const' IDENT ':' type '=' expr ';' ;
```

### 16.6 Statements

```
statement     = let_stmt
              | expr_stmt
              | defer_stmt
              | item ;

let_stmt      = 'let' 'weak'? 'mut'? pattern ( ':' type )? '=' expr ';' ;
expr_stmt     = expr ';' ;
defer_stmt    = 'defer' block_expr ;
```

### 16.7 Expressions

```
expr          = literal
              | path_expr
              | block_expr
              | if_expr
              | match_expr
              | for_expr
              | while_expr
              | loop_expr
              | closure_expr
              | struct_expr
              | array_expr
              | tuple_expr
              | async_block
              | expr '.' IDENT                   /* field access        */
              | expr '.' INT_LIT                 /* tuple field access  */
              | expr '.' 'await'                 /* postfix await       */
              | expr '[' expr ']'                /* index               */
              | expr '(' call_args? ')' trailing_lambda?  /* call      */
              | expr '|>' expr                   /* pipe                */
              | expr '?'                          /* error propagation  */
              | '-' expr                          /* unary negation     */
              | '!' expr                          /* logical not        */
              | 'await' expr                     /* prefix await        */
              | expr 'as' type                    /* type cast          */
              | expr BINOP expr                   /* binary operator    */
              | 'return' expr?                    /* return             */
              | 'break' label? expr?              /* break              */
              | 'continue' label?                 /* continue           */
              | '(' expr ')'                      /* parenthesized     */
              | if_let_expr
              | while_let_expr ;

async_block   = 'async' block_expr ;

block_expr    = '{' statement* expr? '}' ;

if_expr       = 'if' expr block_expr ( 'else' ( if_expr | block_expr ) )? ;
if_let_expr   = 'if' 'let' pattern '=' expr block_expr
                ( 'else' block_expr )? ;

match_expr    = 'match' expr '{' match_arm ( ',' match_arm )* ','? '}' ;
match_arm     = pattern ( 'if' expr )? '=>' expr ;

for_expr      = 'for' pattern 'in' expr block_expr ;
while_expr    = 'while' expr block_expr ;
while_let_expr = 'while' 'let' pattern '=' expr block_expr ;
loop_expr     = 'loop' block_expr ;

closure_expr  = '|' closure_params? '|' ( '->' type )? ( expr | block_expr ) ;
closure_params = closure_param ( ',' closure_param )* ;
closure_param  = IDENT ( ':' type )? ;

trailing_lambda = '{' ( trailing_params '->' )? statement* expr? '}' ;
trailing_params = IDENT ( ',' IDENT )* ;

struct_expr   = path '{' struct_field_init ( ',' struct_field_init )* ','? '}' ;
struct_field_init = '...' expr | IDENT ':' expr | IDENT ;

array_expr    = '[' ( array_elem ( ',' array_elem )* ','? )? ']'
              | '[' expr ';' expr ']' ;
array_elem    = '...' expr | expr ;

tuple_expr    = '(' expr ',' ( expr ( ',' expr )* ','? )? ')' ;

call_args     = call_arg ( ',' call_arg )* ','? ;
call_arg      = ( IDENT ':' )? expr ;
```

### 16.8 Patterns

```
pattern       = literal_pattern
              | binding_pattern
              | '_'
              | tuple_pattern
              | struct_pattern
              | enum_pattern
              | or_pattern
              | range_pattern
              | '(' pattern ')' ;

literal_pattern = INT_LIT | FLOAT_LIT | CHAR_LIT | STRING_LIT | BOOL_LIT ;
binding_pattern = 'mut'? IDENT ;
tuple_pattern   = '(' pattern ( ',' pattern )* ','? ')' ;
struct_pattern  = path '{' field_pattern ( ',' field_pattern )* ( ',' '..' )? '}' ;
field_pattern   = IDENT ':' pattern | IDENT ;
enum_pattern    = path ( '(' pattern ( ',' pattern )* ')' )?
                | path '{' field_pattern ( ',' field_pattern )* '}' ;
or_pattern      = pattern '|' pattern ( '|' pattern )* ;
range_pattern   = expr '..' expr | expr '..=' expr ;
```

### 16.9 Extern Blocks

```
extern_block      = 'extern' ( STRING_LIT )? '{' extern_item* '}' ;

extern_item       = extern_fn_decl
                  | extern_static_decl ;

extern_fn_decl   = attribute_list? 'fn' IDENT '(' extern_params? ')' ( '->' type )? ';' ;

extern_params    = extern_param ( ',' extern_param )* ( ',' '...' )? ','? ;
extern_param     = IDENT ':' type ;

extern_static_decl = attribute_list? 'let' IDENT ':' type ';' ;
```

### 16.10 Actors

```
actor_def     = visibility? 'actor' IDENT generics?
                '{' actor_member* '}' ;

actor_member  = struct_field
              | fn_def ;
```

---

## 17. Concurrency

### 17.1 Overview

nudl provides single-threaded cooperative concurrency in version 1. Async
functions are compiled as state machines (following Rust's technique), and
structured concurrency (following Swift's model) ensures that child tasks
cannot outlive their parent scope.

Key properties:

- **Single-threaded.** All async tasks run on a single thread via cooperative
  scheduling. ARC remains non-atomic.
- **State machine compilation.** Each `async fn` is compiled into a state
  machine that suspends at `.await` points and resumes when the awaited
  future completes.
- **Structured concurrency.** Spawned tasks are bound to the scope that
  spawned them. Exiting a scope before spawned tasks complete triggers
  cancellation.
- **Built-in executor.** The async runtime includes a built-in, implicit
  executor. `async fn main()` is a valid program entry point — no user
  setup or explicit executor configuration is required. The executor is
  started automatically when the program's `main` function is `async`.

### 17.2 Async Functions

```
fn_def  = visibility? 'comptime'? 'async'? 'fn' identifier generics?
          '(' fn_params? ')' ( '->' type )? where_clause? block_expr ;
```

An `async fn` declares a function whose body can suspend execution at `.await`
points. The function does not execute when called — instead, it returns a
`Future<T>` to the caller, where `T` is the declared return type.

```nudl
async fn fetch_data(url: string) -> string {
    let response = http_get(url).await;
    response.body()
}

// The type of fetch_data is: (string) -> Future<string>
let future = fetch_data("https://example.com");  // does not execute yet
let data = future.await;                          // executes and suspends
```

**Restrictions:**

- `async` and `comptime` are mutually exclusive. Combining them is a compile
  error.
- The return type annotation is the "inner" type: `async fn foo() -> i32`
  has effective type `() -> Future<i32>`.

### 17.3 Await Expressions

nudl supports both **postfix** and **prefix** forms of `await`:

```
await_expr  = expression '.' 'await'       // postfix (precedence 14)
            | 'await' expression ;          // prefix  (low, like 'return')
```

**Postfix `.await`** has high precedence (14, alongside `.` and `?`), enabling
mid-expression chaining:

```nudl
let body = fetch(url).await.json().await;
```

**Prefix `await`** has low precedence (like `return`), capturing the entire
following expression:

```nudl
await fetch(url).await.json()
// Parses as: await ((fetch(url)).await.json())
```

**Rules:**

- `.await` and `await` may only appear inside an `async fn` or `async` block.
  Using them elsewhere is a compile error.
- `.await` on a `Future<T>` yields a value of type `T`, suspending the
  current task until the future resolves.

### 17.4 Async Blocks

```
async_block  = 'async' block_expr ;
```

An `async` block produces a `Future<T>` where `T` is the type of the block's
tail expression. Like closures, async blocks capture variables from the
enclosing scope by ARC reference.

```nudl
let name = "world";
let greeting: Future<string> = async {
    let data = fetch_greeting().await;
    f"{data}, {name}!"
};
let message = greeting.await;
```

### 17.5 The Future Type

`Future<T>` is a built-in generic reference type representing a value that will
be available after an asynchronous computation completes.

- `Future<T>` is heap-allocated and ARC'd.
- It is not directly constructable except through `async fn` or `async { }`.
- Polling a future to completion is handled by the runtime executor (see
  Section 17.6).

### 17.6 Task Spawning and Structured Concurrency

#### 17.6.1 Task.spawn

`Task.spawn` starts a new concurrent task from an async expression:

```nudl
let handle = Task.spawn(async { compute_result().await });
let result = handle.await;
```

- `Task.spawn` returns `TaskHandle<T>`, which itself implements `Future<T>`.
- The spawned task begins executing concurrently (cooperatively interleaved
  with the parent task).
- The spawned task's lifetime is bound to the scope that spawned it
  (structured concurrency). If the scope exits before the task completes,
  the task is cancelled.

#### 17.6.2 Task Groups

Task groups enable structured fan-out patterns where multiple tasks execute
concurrently and their results are collected:

```nudl
let results = Task.group { group ->
    for url in urls {
        group.spawn(async { fetch(url).await });
    }
};
// results: string[]  (all child results collected)
```

- `Task.group` takes a closure that receives a `TaskGroup<T>` handle.
- The group awaits all spawned children before returning.
- Results are collected in spawn order.
- If any child panics, remaining children are cancelled and the panic
  propagates to the parent.

#### 17.6.3 Cancellation

Cancellation uses **drop-at-next-suspension-point** semantics. When a task is
cancelled (e.g., because its parent scope exits), the cancellation is detected
at the next `.await` suspension point. At that point, the task's stack unwinds
automatically: `defer` blocks and `Drop` implementations execute normally
during the unwind, ensuring cleanup runs reliably.

Between suspension points, tasks may also check cancellation proactively:

- `Task.is_cancelled() -> bool`: checks whether the current task has been
  cancelled.
- `Task.check_cancelled() -> Result<(), CancelledError>`: returns `Err` if
  cancelled, designed for use with `?`.

```nudl
async fn long_running() -> Result<i32, Error> {
    for i in 0..1000000 {
        // At this .await, cancellation is auto-detected and stack unwinds
        process(i).await;
    }
    Ok(42)
}

async fn with_manual_check() -> Result<(), Error> {
    for i in 0..1000000 {
        // Can also check between await points for long CPU-bound sections
        Task.check_cancelled()?;
        expensive_sync_work(i);
    }
    Ok(())
}
```

When a cancelled task unwinds:
1. The current `.await` returns a cancellation signal (not an `Err` value).
2. The stack unwinds through all frames, executing `defer` blocks in LIFO order.
3. `Drop` implementations run normally for all values going out of scope.
4. The task is marked complete with a cancellation status.

### 17.6.4 Async Drop Restriction

Drop implementations are always synchronous. Async operations cannot be performed during drop. For resources requiring async cleanup, use `defer { resource.close().await; }` before the resource goes out of scope.

### 17.7 Actors

#### 17.7.1 Declaration

Actors are concurrent objects with isolated mutable state. All access to an
actor's fields goes through its methods, which are implicitly serialized.

```
actor_def    = visibility? 'actor' identifier generics? '{' actor_member* '}' ;
actor_member = struct_field | fn_def ;
```

```nudl
actor Counter {
    value: i32,

    fn new() -> Counter {
        Counter { value: 0 }
    }

    fn increment(mut self) {
        self.value += 1;
    }

    fn get(self) -> i32 {
        self.value
    }
}
```

#### 17.7.2 Isolation Rules

- **Fields are never directly accessible from outside.** All access goes
  through the actor's methods.
- **External calls are implicitly async.** Calling an actor method from
  outside returns `Future<T>`:

  ```nudl
  let counter = Counter::new();
  counter.increment().await;       // returns Future<()>, must be awaited
  let val = counter.get().await;   // returns Future<i32>
  ```

- **Internal calls are synchronous.** Within the actor's own methods, `self`
  access is direct and synchronous — no `.await` needed.
- **Actors are reference types.** They are heap-allocated and ARC'd.
  Assigning an actor increments the reference count (aliases share the same
  actor instance and its message queue).
- **No returning references to internal state.** Actor methods must not return
  references to fields or internal data structures. A method that attempts to
  return a reference type that aliases the actor's own state is a compile error.
  This ensures that the actor's isolation boundary is not violated through
  returned references.

  ```nudl
  actor Database {
      entries: Map<string, string>,

      // ERROR: returning a reference to internal state
      // fn get_entries(self) -> Map<string, string> {
      //     self.entries   // compile error: leaks actor state
      // }

      // OK: return a clone (independent copy)
      fn get_entries(self) -> Map<string, string> {
          self.entries.clone()
      }

      // OK: return a value type extracted from state
      fn get(self, key: string) -> Option<string> {
          self.entries.get(key).map { it.clone() }
      }
  }
  ```

- **Can hold references to other actors.** An actor may store references to
  other actors in its fields. Calling methods on those actors goes through the
  normal async message-passing protocol.

#### 17.7.3 Message Ordering

External calls to an actor are dispatched with **FIFO-per-sender** ordering: messages from the same sender are processed in the order they were sent. No ordering is guaranteed between messages from different senders.

---

## 18. Appendices

### 18.1 Implementation Notes

#### v1 Optimization Passes

The v1 compiler applies the following optimization passes to SSA bytecode:

- **Retain/release elision** — removes balanced ARC operations on the same register
- **Constant folding** — evaluates constant expressions at compile time
- **Dead code elimination** — removes unreachable blocks and unused computations
- **Basic inlining** — inlines small, non-recursive functions at call sites

Additional optimizations (copy propagation, loop-invariant code motion, vectorization) are deferred to future versions.

### Appendix A: Keyword Table

| Keyword     | Description                                                    |
|-------------|----------------------------------------------------------------|
| `actor`     | Define a concurrent actor type                                 |
| `as`        | Type cast operator; import aliasing                            |
| `async`     | Declare an async function or async block                       |
| `await`     | Suspend until a future completes (prefix form)                 |
| `break`     | Exit a loop, optionally with a value                           |
| `comptime`  | Compile-time evaluation context                                |
| `const`     | Declare a compile-time constant                                |
| `continue`  | Skip to the next loop iteration                                |
| `defer`     | Schedule a block to run on scope exit                          |
| `dyn`       | Dynamic dispatch type constructor                              |
| `else`      | Alternative branch of if/if-let                                |
| `enum`      | Define a sum type (algebraic data type)                        |
| `extern`    | Declare foreign function or static from a C library            |
| `false`     | Boolean literal                                                |
| `fn`        | Define a function                                              |
| `for`       | Iterate over a collection or range                             |
| `if`        | Conditional branch                                             |
| `impl`      | Define methods for a type or implement an interface            |
| `import`    | Bring items from another module into scope                     |
| `in`        | Separator in for loops (`for x in collection`)                 |
| `interface` | Define a set of required/default methods for bounded polymorphism |
| `let`       | Introduce a variable binding                                   |
| `loop`      | Infinite loop                                                  |
| `match`     | Pattern matching expression                                    |
| `mut`       | Mark a binding or receiver as mutable                          |
| `pub`       | Make an item publicly visible                                  |
| `quote`     | Produce an AST fragment in comptime context                    |
| `return`    | Return a value from a function                                 |
| `self`      | The receiver of a method                                       |
| `Self`      | The implementing type in interfaces and impl blocks            |
| `struct`    | Define a product type                                          |
| `true`      | Boolean literal                                                |
| `type`      | Define a type alias                                            |
| `weak`      | Create a weak reference (does not prevent deallocation)        |
| `where`     | Specify additional generic bounds                              |
| `while`     | Loop with a condition                                          |

### Appendix B: Operator Precedence Table

Ordered from highest precedence (binds tightest) to lowest:

| Prec | Category      | Operators                              | Associativity |
|------|---------------|----------------------------------------|---------------|
| 15   | Primary       | `.` `[]` `()`                          | Left          |
| 14   | Postfix       | `?` `.await`                           | Postfix       |
| 13   | Prefix        | `-` (unary) `!`                        | Prefix        |
| 12   | Cast          | `as`                                   | Left          |
| 11   | Multiplicative| `*` `/` `%`                            | Left          |
| 10   | Additive      | `+` `-`                                | Left          |
| 9    | Shift         | `<<` `>>`                              | Left          |
| 8    | Bitwise AND   | `&`                                    | Left          |
| 7    | Bitwise XOR   | `^`                                    | Left          |
| 6    | Bitwise OR    | `\|`                                   | Left          |
| 5    | Comparison    | `==` `!=` `<` `>` `<=` `>=`           | None          |
| 4    | Logical AND   | `&&`                                   | Left          |
| 3    | Logical OR    | `\|\|`                                 | Left          |
| 2    | Range         | `..` `..=`                             | None          |
| 1    | Pipe          | `\|>`                                  | Left          |
| 0    | Assignment    | `=` `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` | Right         |

"None" associativity means chaining is not permitted: `a < b < c` is a syntax
error. Prefix `await` is not in this table — like `return`, it is a prefix
keyword expression, not an operator.

### Appendix C: Escape Sequences

Valid escape sequences in string, format string, and character literals:

| Escape     | Character                | Unicode    |
|------------|--------------------------|------------|
| `\n`       | Line feed                | U+000A     |
| `\r`       | Carriage return          | U+000D     |
| `\t`       | Horizontal tab           | U+0009     |
| `\\`       | Backslash                | U+005C     |
| `\"`       | Double quote             | U+0022     |
| `\'`       | Single quote             | U+0027     |
| `\0`       | Null                     | U+0000     |
| `\xNN`     | Byte value (2 hex digits)| 0x00-0xFF  |
| `\u{N..N}` | Unicode scalar (1-6 hex) | U+0000-U+10FFFF |

The `\'` escape is valid only in character literals. The `\"` escape is valid
only in string and format string literals.

### Appendix D: Reserved Words

The following words are reserved for potential future use and cannot be used as
identifiers. Using them as identifiers is a compile-time error:

```
crate     macro     mod       move      override
priv      ref       static    super     trait
try       unsafe    use       virtual   yield
```

These words are not currently keywords but may become keywords in future versions
of nudl. Note that `async`, `await`, `actor`, `const`, and `extern` were
promoted from reserved words to keywords when their respective features were
added.

### Appendix E: Target Platform Roadmap

| Version | Target | Architecture | Binary Format | Status |
|---------|--------|-------------|---------------|--------|
| v1 | `aarch64-apple-darwin` | ARM64 | Mach-O | Planned |
| v1 | `aarch64-linux-gnu` | ARM64 | ELF | Planned |
| v2 | `x86_64-apple-darwin` | x86-64 | Mach-O | Future |
| v2 | `x86_64-linux-gnu` | x86-64 | ELF | Future |

The v1 compiler targets ARM64 exclusively. The SSA bytecode IR is
architecture-independent, so adding x86-64 support requires only a new backend
(`nudl-backend-x86_64`) that consumes the same SSA IR and produces x86-64
machine code. The packer crates already support the relevant binary formats.

---

*End of specification.*
