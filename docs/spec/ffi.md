## 19. Foreign Function Interface

### 19.1 Overview

nudl provides a Foreign Function Interface (FFI) for calling functions defined
in C libraries. FFI enables interaction with the operating system, system
libraries, and existing C codebases.

**Design principles:**

1. **No unsafe system.** Extern function calls are regular function calls. There
   is no `unsafe` keyword or block. The compiler enforces FFI type safety at the
   declaration boundary — only FFI-compatible types may appear in extern
   signatures.

2. **Minimal new types.** FFI introduces three new built-in types: `RawPtr`,
   `MutRawPtr`, and `CStr`. These are sufficient for handle-passing patterns
   without exposing pointer arithmetic or dereference operations.

3. **Explicit linking.** Libraries are specified either inline in extern blocks
   or in `nudl.toml`. The compiler resolves library names to platform-specific
   paths at link time.

4. **ARC boundary.** nudl's ARC-managed objects never cross the FFI boundary.
   Extern functions operate on FFI-safe types only. The programmer is
   responsible for managing C-side resource lifetimes (typically via the `Drop`
   interface on a wrapper struct).

### 19.2 Extern Blocks

#### 19.2.1 Syntax

```
extern_block      = 'extern' ( STRING_LIT )? '{' extern_item* '}' ;

extern_item       = extern_fn_decl
                  | extern_static_decl ;

extern_fn_decl   = attribute_list? 'fn' IDENT '(' extern_params? ')' ( '->' type )? ';' ;

extern_params    = extern_param ( ',' extern_param )* ','? ;
extern_param     = IDENT ':' type ;

extern_static_decl = attribute_list? 'let' IDENT ':' type ';' ;
```

An extern block declares foreign symbols. The optional string literal specifies
the library to link (see Section 19.4). If omitted, the symbols are expected to
be provided by the default C runtime (libc/libSystem).

```nudl
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
    fn read(fd: i32, buf: MutRawPtr, count: u64) -> i64;
    fn close(fd: i32) -> i32;
}

extern "libz" {
    fn compress(
        dest: MutRawPtr,
        dest_len: MutRawPtr,
        source: RawPtr,
        source_len: u64,
    ) -> i32;
}
```

#### 19.2.2 Name Binding

By default, the nudl function name is used as the C symbol name. The
`#[link_name]` attribute overrides this:

```nudl
extern {
    #[link_name = "isatty"]
    fn is_terminal(fd: i32) -> i32;
}

// Calls the C function `isatty`, but is known as `is_terminal` in nudl code.
is_terminal(1);
```

#### 19.2.3 Calling Rules

Extern function calls differ from regular nudl function calls:

1. **All arguments are positional.** Named argument syntax is not supported.
2. **No default parameters.** Every parameter must be supplied at each call site.
3. **No optional parameters.** The `?` suffix is not allowed.
4. **No trailing lambda.** The trailing lambda syntax is not applicable.
5. **No method syntax.** Extern functions cannot be called via `.` syntax.
6. **No generics.** Extern function declarations cannot have type parameters.

```nudl
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

// Correct:
let n = write(1, data, len);

// ERROR: named arguments not supported in extern calls
// let n = write(1, buf: data, count: len);
```

#### 19.2.4 Extern Statics

Extern blocks may declare read-only static values imported from C:

```nudl
extern {
    let errno: i32;
}
```

Extern statics are read-only in nudl. Attempting to assign to an extern static
is a compile error. The declared type must be FFI-safe (see Section 19.3.5).

#### 19.2.5 Visibility

Extern blocks follow standard visibility rules. By default, all declarations
within an extern block are private to the enclosing module. The `pub` keyword
may be applied to individual declarations:

```nudl
extern {
    pub fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
    fn _internal_helper(x: i32) -> i32;  // private
}
```

#### 19.2.6 Variadic Functions

C variadic functions can be declared using `...` as the final parameter in an extern block:

```nudl
extern {
    fn printf(fmt: CStr, ...) -> i32;
    fn snprintf(buf: MutRawPtr, size: u64, fmt: CStr, ...) -> i32;
}
```

Variadic arguments must be FFI-safe types (see Section 19.3). The compiler performs standard C variadic promotion rules:

- `i8`, `i16` promote to `i32`
- `u8`, `u16` promote to `u32`
- `f32` promotes to `f64`
- All other FFI-safe types are passed as-is

Example usage:

```nudl
let count = printf("Hello, %s! You are %d years old.\0".as_cstr(), name.as_cstr(), age);
```

Variadic extern functions follow the same rules as non-variadic extern functions: all arguments are positional, no named arguments, no default values, no trailing lambdas.

### 19.3 FFI Types

#### 19.3.1 Primitive Type Mapping

nudl primitives map directly to C types on AArch64:

| nudl type | C type (AArch64) | Size | Notes |
|-----------|-----------------|------|-------|
| `i8` | `int8_t` / `char` | 1 | Signed byte |
| `i16` | `int16_t` | 2 | |
| `i32` | `int32_t` | 4 | |
| `i64` | `int64_t` | 8 | |
| `u8` | `uint8_t` | 1 | |
| `u16` | `uint16_t` | 2 | |
| `u32` | `uint32_t` | 4 | |
| `u64` | `uint64_t` / `size_t` | 8 | `size_t` is 8 bytes on AArch64 |
| `f32` | `float` | 4 | IEEE 754 binary32 |
| `f64` | `double` | 8 | IEEE 754 binary64 |
| `bool` | `_Bool` | 1 | C99 `_Bool` / C++ `bool` |
| `()` | `void` | 0 | Return type only |

The unit type `()` is permitted only as a return type in extern declarations
(equivalent to C `void`). It cannot appear as a parameter type.

#### 19.3.2 Opaque Pointer Types: `RawPtr` and `MutRawPtr`

`RawPtr` and `MutRawPtr` are built-in opaque pointer types representing `const
void*` and `void*` respectively. They are value types — assignment copies the
pointer value (not the pointee).

**Operations:**

| Expression | Type | Description |
|---|---|---|
| `RawPtr.null()` | `RawPtr` | The null pointer constant |
| `MutRawPtr.null()` | `MutRawPtr` | The null pointer constant |
| `ptr.is_null()` | `bool` | Returns `true` if the pointer is null |
| `ptr.as_mut()` | `MutRawPtr` | Converts `RawPtr` to `MutRawPtr` |
| `ptr.as_const()` | `RawPtr` | Converts `MutRawPtr` to `RawPtr` |
| `ptr == other` | `bool` | Pointer equality (address comparison) |
| `ptr != other` | `bool` | Pointer inequality |

**Restrictions:**

- No dereference. There is no `*ptr` or `ptr.deref()` operation.
- No pointer arithmetic. There is no `ptr + offset` or `ptr.offset(n)` operation.
- No casting to or from integer types.

`RawPtr` and `MutRawPtr` are handles — opaque tokens passed between nudl and C
code. The only way to create a non-null pointer is by receiving one from a C
function.

```nudl
extern {
    fn malloc(size: u64) -> MutRawPtr;
    fn free(ptr: MutRawPtr);
    fn memcpy(dest: MutRawPtr, src: RawPtr, n: u64) -> MutRawPtr;
}

let buf = malloc(1024);
if buf.is_null() {
    panic("allocation failed");
}
// Use buf via C functions...
free(buf);
```

`RawPtr` and `MutRawPtr` implement `Eq` (pointer equality) but do not
implement `Ord`, `Clone`, `Printable`, or any other built-in interface.

#### 19.3.3 C String Type: `CStr`

`CStr` is a built-in type representing a pointer to a null-terminated C string
(`const char*`). It is a value type — assignment copies the pointer, not the
string data.

**Operations:**

| Expression | Type | Description |
|---|---|---|
| `CStr.null()` | `CStr` | The null CStr (null pointer) |
| `cstr.is_null()` | `bool` | Returns `true` if the CStr is null |
| `cstr.as_raw()` | `RawPtr` | Converts to a raw pointer |
| `cstr.to_string()` | `Result<string, Error>` | Copies the C string data into a new nudl string; returns `Err` on invalid UTF-8 |
| `s.as_cstr()` | `CStr` | Borrows the nudl string as a CStr (see lifetime rules) |

**`as_cstr()` lifetime rules:**

The `as_cstr()` method on `string` returns a `CStr` that points into the
string's internal buffer with an appended null terminator. The resulting `CStr`
is valid only while the source string is alive and unmodified. The compiler does
not enforce this lifetime statically — it is the programmer's responsibility to
ensure the source string outlives the `CStr`.

A common safe pattern is to use `as_cstr()` inline in a call expression:

```nudl
extern {
    fn puts(s: CStr) -> i32;
}

let message = "Hello, world!";
puts(message.as_cstr());  // Safe: string outlives the call
```

Storing a `CStr` obtained from `as_cstr()` and using it after the source
string has been deallocated is undefined behavior.

**`to_string()` semantics:**

`CStr.to_string()` scans the C string for a null terminator, copies the bytes
into a new nudl-managed string, and validates UTF-8. If the C string contains
invalid UTF-8, the function returns `Err` with a description of the invalid
byte sequence. This allows callers to handle encoding issues gracefully rather
than crashing.

Calling `to_string()` on a null `CStr` is a runtime error.

#### 19.3.4 Types Not Allowed in Extern Declarations

The following types may not appear in extern function signatures or extern
static declarations:

- `string` — Use `CStr` instead.
- Structs and enums — No ABI-compatible layout guarantee in v1.
- Dynamic arrays (`T[]`) — Reference type with ARC header.
- Fixed-size arrays (`[T; N]`) — No C equivalent in function signatures.
- Maps (`Map<K, V>`) — Reference type.
- Closures / function types (`(T) -> U`) — See Section 19.6 for callbacks.
- `Future<T>` — Runtime-managed type.
- `dyn Interface` — Fat pointer with nudl-specific vtable layout.
- `weak T` — Weak reference wrapper.
- `Option<T>`, `Result<T, E>` — Enum types.

Using a disallowed type in an extern declaration is a compile error:

```
error[E0903]: type `string` is not FFI-safe
  --> src/main.nudl:3:18
   |
3  |     fn puts(s: string) -> i32;
   |                ^^^^^^ not allowed in extern declarations
   |
   = help: use `CStr` for null-terminated C strings
```

#### 19.3.5 FFI-Safe Type Summary

| Type | FFI-safe | C equivalent (AArch64) |
|------|----------|----------------------|
| `i8`, `i16`, `i32`, `i64` | Yes | `int8_t` .. `int64_t` |
| `u8`, `u16`, `u32`, `u64` | Yes | `uint8_t` .. `uint64_t` |
| `f32`, `f64` | Yes | `float`, `double` |
| `bool` | Yes | `_Bool` |
| `()` | Yes | `void` (return only) |
| `RawPtr` | Yes | `const void*` |
| `MutRawPtr` | Yes | `void*` |
| `CStr` | Yes | `const char*` |
| `string` | No | — |
| structs, enums | No | — |
| `T[]`, `[T; N]`, `Map<K,V>` | No | — |
| closures, `dyn I`, `Future<T>` | No | — |

### 19.4 Library Linking

#### 19.4.1 Inline Library String

The library name in an extern block specifies a shared library to link:

```nudl
extern "libz" {
    fn compress(dest: MutRawPtr, dest_len: MutRawPtr,
                source: RawPtr, source_len: u64) -> i32;
}
```

The compiler resolves the library name to a platform-specific path:

| Library string | macOS (Darwin) | Linux (ELF) |
|---|---|---|
| `"libz"` | `-lz` (links `libz.dylib`) | `-lz` (links `libz.so`) |
| `"libsqlite3"` | `-lsqlite3` | `-lsqlite3` |

The `lib` prefix in the library string is stripped to produce the linker flag.
For example, `"libz"` becomes `-lz`.

#### 19.4.2 Framework Linking (macOS)

On macOS, system frameworks are linked using the `framework:` prefix:

```nudl
extern "framework:CoreFoundation" {
    fn CFRelease(cf: RawPtr);
}
```

This produces the linker flag `-framework CoreFoundation`.

#### 19.4.3 Default Linking

An extern block without a library string links against the default C runtime:

- **macOS:** `libSystem.dylib` (provides libc, libm, and system calls)
- **Linux:** `libc.so` (glibc or musl)

Since the nudl runtime requires `malloc`/`free` for ARC allocation, libc is
always linked. Extern blocks without a library string add no additional link
dependencies.

#### 19.4.4 Build Configuration: `[build.link]`

For advanced linking needs, the `[build.link]` table in `nudl.toml` specifies
additional search paths and libraries:

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

Libraries specified in `[build.link]` are linked in addition to any libraries
specified inline in extern blocks. Duplicate library names are deduplicated.

### 19.5 Interaction with ARC

#### 19.5.1 Boundary Rules

nudl's ARC-managed objects do not cross the FFI boundary. Extern functions
accept and return only FFI-safe types. There is no mechanism to pass a nudl
struct, enum, string, or array to C code.

To transfer data to C code, the programmer must:

1. Extract the relevant fields into FFI-safe types.
2. Pass those values to the extern function.
3. Interpret the return value and construct nudl objects as needed.

```nudl
struct Config {
    host: string,
    port: u16,
}

extern {
    fn connect(host: CStr, port: u16) -> i32;
}

fn open_connection(config: Config) -> i32 {
    connect(config.host.as_cstr(), config.port)
}
```

#### 19.5.2 CStr Lifetime at the ARC Boundary

When `as_cstr()` is called on a nudl string, the resulting `CStr` points into
the string's internal buffer. The string's reference count ensures the buffer
remains allocated for the duration of the current scope. However, if the `CStr`
is stored and the string's last reference is dropped, the `CStr` becomes a
dangling pointer.

**Safe pattern — inline use:**

```nudl
puts(message.as_cstr());  // string lives through the call
```

**Dangerous pattern — stored CStr:**

```nudl
let cstr = message.as_cstr();
drop(message);    // string deallocated
puts(cstr);       // UNDEFINED BEHAVIOR: dangling pointer
```

The compiler emits a warning when a `CStr` produced by `as_cstr()` is bound to
a variable rather than used inline (see W0901 in Section 19.10).

#### 19.5.3 Resource Cleanup with Drop

C resources that require explicit cleanup (file descriptors, handles, allocated
memory) should be wrapped in a struct that implements `Drop`:

```nudl
extern {
    fn sqlite3_open(filename: CStr, db: MutRawPtr) -> i32;
    fn sqlite3_close(db: RawPtr) -> i32;
}

struct Database {
    handle: RawPtr,
}

impl Database {
    fn open(path: string) -> Result<Database, string> {
        let mut handle = MutRawPtr.null();
        let rc = sqlite3_open(path.as_cstr(), handle);
        if rc != 0 {
            return Err("failed to open database");
        }
        Ok(Database { handle: handle.as_const() })
    }
}

impl Drop for Database {
    fn drop(mut self) {
        if !self.handle.is_null() {
            sqlite3_close(self.handle);
        }
    }
}
```

### 19.6 Callbacks: Passing nudl Functions to C

Some C libraries accept function pointers as callbacks. nudl supports this via
the `#[extern_callable]` attribute and the `extern_fn_ptr()` intrinsic.

#### 19.6.1 The `#[extern_callable]` Attribute

A function annotated with `#[extern_callable]` is compiled with the C calling
convention and may be passed to C code as a function pointer.

```nudl
#[extern_callable]
fn compare_ints(a: RawPtr, b: RawPtr) -> i32 {
    // Implementation using C-compatible types only
    0
}
```

**Restrictions:**

1. All parameter types and the return type must be FFI-safe.
2. The function must not be a closure. Only named, top-level or module-level
   functions may be `#[extern_callable]`.
3. The function must not be `async`.
4. The function must not panic. If an `#[extern_callable]` function panics, the
   program aborts immediately — the panic does not propagate across the C frame
   boundary.
5. The function must not be `comptime`.
6. The function must not have generic type parameters.

#### 19.6.2 The `extern_fn_ptr()` Intrinsic

The `extern_fn_ptr()` built-in function converts a reference to an
`#[extern_callable]` function into a `RawPtr` suitable for passing to C:

```nudl
extern {
    fn qsort(base: MutRawPtr, nel: u64, width: u64, compar: RawPtr);
}

#[extern_callable]
fn compare(a: RawPtr, b: RawPtr) -> i32 {
    // ...
    0
}

// Pass the function pointer to C:
qsort(data, count, elem_size, extern_fn_ptr(compare));
```

`extern_fn_ptr(f)` requires that `f` is a function with the `#[extern_callable]`
attribute. Passing a non-`#[extern_callable]` function is a compile error.

The returned `RawPtr` represents a C function pointer (`void*`). The C library
is expected to cast it to the appropriate function pointer type.

### 19.7 Interaction with Comptime

#### 19.7.1 No Extern Calls at Comptime

Extern functions cannot be called in comptime context. The comptime VM is a
sandboxed environment with no access to system libraries or OS facilities.
Attempting to call an extern function in a comptime block is a compile error:

```nudl
extern {
    fn getpid() -> i32;
}

comptime {
    let pid = getpid();  // ERROR[E0906]: extern function `getpid` cannot
                         //   be called in comptime context
}
```

#### 19.7.2 Generating Extern Declarations

Comptime code may generate extern blocks via `quote`. This enables patterns
where a comptime function reads a configuration and emits the appropriate
extern declarations:

```nudl
comptime fn declare_math_functions(names: string[]) {
    for name in names {
        quote {
            extern {
                #[link_name = ${name}]
                fn ${name}(x: f64) -> f64;
            }
        }
    }
}

comptime {
    declare_math_functions(["sin", "cos", "tan", "sqrt", "log"]);
}

// Now sin, cos, tan, sqrt, log are available as functions.
let x = sin(3.14159);
```

### 19.8 Common Patterns

#### 19.8.1 Wrapping a C Library

A typical pattern for wrapping a C library involves three layers:

1. **Raw extern declarations** — direct mapping of C functions.
2. **Safe wrapper struct** — manages lifetime via `Drop`.
3. **Ergonomic API** — nudl-idiomatic methods on the wrapper.

```nudl
// Layer 1: Raw declarations
extern "libsqlite3" {
    fn sqlite3_open(filename: CStr, ppDb: MutRawPtr) -> i32;
    fn sqlite3_close(db: RawPtr) -> i32;
    fn sqlite3_exec(
        db: RawPtr,
        sql: CStr,
        callback: RawPtr,
        arg: MutRawPtr,
        errmsg: MutRawPtr,
    ) -> i32;
    fn sqlite3_free(ptr: MutRawPtr);
}

// Layer 2: Safe wrapper
struct SqliteDb {
    ptr: RawPtr,
}

impl Drop for SqliteDb {
    fn drop(mut self) {
        if !self.ptr.is_null() {
            sqlite3_close(self.ptr);
        }
    }
}

// Layer 3: Ergonomic API
impl SqliteDb {
    fn open(path: string) -> Result<SqliteDb, string> {
        let mut db_ptr = MutRawPtr.null();
        let rc = sqlite3_open(path.as_cstr(), db_ptr);
        if rc != 0 {
            return Err(f"sqlite3_open failed with code {rc}");
        }
        Ok(SqliteDb { ptr: db_ptr.as_const() })
    }

    fn execute(self, sql: string) -> Result<(), string> {
        let mut errmsg = MutRawPtr.null();
        let rc = sqlite3_exec(
            self.ptr,
            sql.as_cstr(),
            RawPtr.null(),
            MutRawPtr.null(),
            errmsg,
        );
        if rc != 0 {
            // In a real implementation, read errmsg and free it
            sqlite3_free(errmsg);
            return Err(f"sqlite3_exec failed with code {rc}");
        }
        Ok(())
    }
}

// Usage:
fn main() {
    let db = SqliteDb::open("test.db").unwrap();
    db.execute("CREATE TABLE IF NOT EXISTS users (name TEXT)").unwrap();
    db.execute("INSERT INTO users VALUES ('Alice')").unwrap();
    // db is automatically closed when it goes out of scope
}
```

#### 19.8.2 Calling libc

For simple system calls, extern declarations can be used directly without a
wrapper:

```nudl
extern {
    fn getpid() -> i32;
    fn getenv(name: CStr) -> CStr;
    fn exit(status: i32);
}

fn main() {
    let pid = getpid();
    println(f"PID: {pid}");

    let home = getenv("HOME".as_cstr());
    if !home.is_null() {
        match home.to_string() {
            Ok(s) => println(f"HOME: {s}"),
            Err(e) => println(f"HOME contains invalid UTF-8: {e.message()}"),
        }
    }
}
```

#### 19.8.3 Opaque Handle Pattern

Many C libraries use opaque handles — pointers to implementation-internal
structures. The opaque pointer types map naturally to this pattern:

```nudl
extern "libcurl" {
    fn curl_easy_init() -> MutRawPtr;
    fn curl_easy_cleanup(handle: MutRawPtr);
    fn curl_easy_setopt(handle: MutRawPtr, option: i32, value: RawPtr) -> i32;
    fn curl_easy_perform(handle: MutRawPtr) -> i32;
}

struct CurlHandle {
    ptr: MutRawPtr,
}

impl CurlHandle {
    fn new() -> Result<CurlHandle, string> {
        let ptr = curl_easy_init();
        if ptr.is_null() {
            return Err("curl_easy_init failed");
        }
        Ok(CurlHandle { ptr })
    }

    fn perform(self) -> Result<(), i32> {
        let rc = curl_easy_perform(self.ptr);
        if rc != 0 {
            return Err(rc);
        }
        Ok(())
    }
}

impl Drop for CurlHandle {
    fn drop(mut self) {
        if !self.ptr.is_null() {
            curl_easy_cleanup(self.ptr);
        }
    }
}
```

### 19.9 Grammar Additions

The following productions are added to the grammar (Section 16):

```
extern_block      = 'extern' ( STRING_LIT )? '{' extern_item* '}' ;

extern_item       = extern_fn_decl
                  | extern_static_decl ;

extern_fn_decl   = attribute_list? 'fn' IDENT '(' extern_params? ')' ( '->' type )? ';' ;

extern_params    = extern_param ( ',' extern_param )* ( ',' '...' )? ','? ;
extern_param     = IDENT ':' type ;

extern_static_decl = attribute_list? 'let' IDENT ':' type ';' ;
```

The `item` production (Section 16.1) is extended:

```
item          = fn_def
              | struct_def
              | enum_def
              | interface_def
              | impl_block
              | impl_interface
              | type_alias
              | import_decl
              | comptime_block
              | actor_def
              | extern_block ;
```

### 19.10 Diagnostics

#### 19.10.1 Errors

| Code | Name | Description |
|------|------|-------------|
| E0901 | `non_ffi_safe_type` | A type in an extern declaration is not FFI-safe |
| E0902 | `extern_fn_generic` | An extern function declaration has generic type parameters |
| E0903 | `extern_fn_body` | An extern function declaration has a body (extern functions must use `;`) |
| E0904 | `extern_callable_invalid` | `#[extern_callable]` applied to a closure, async fn, comptime fn, or generic fn |
| E0905 | `extern_fn_ptr_not_callable` | `extern_fn_ptr()` called with a function lacking `#[extern_callable]` |
| E0906 | `extern_call_in_comptime` | An extern function was called in comptime context |

#### 19.10.2 Warnings

| Code | Name | Description |
|------|------|-------------|
| W0901 | `cstr_stored_from_as_cstr` | A `CStr` from `as_cstr()` is stored in a binding rather than used inline |
| W0902 | `unused_extern_fn` | An extern function is declared but never called |

---
