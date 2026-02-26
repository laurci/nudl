# Task 22: Standard Library

## Goal
Implement a minimal standard library written in nudl using extern C calls, providing `std::io`, `std::math`, `std::string`, and core utilities.

## Requirements

### `std::io`
- `pub fn print(s: String)` — print string to stdout (no newline)
- `pub fn println(s: String)` — print string + newline
- `pub fn eprintln(s: String)` — print to stderr + newline
- `pub fn read_line() -> String` — read line from stdin
- Implemented via extern C: `write`, `read` syscalls or `printf`/`fgets`

### `std::math`
- `pub fn abs(x: i64) -> i64`, `pub fn abs_f64(x: f64) -> f64`
- `pub fn min(a: i64, b: i64) -> i64`, `pub fn max(a: i64, b: i64) -> i64`
- `pub fn min_f64(a: f64, b: f64) -> f64`, `pub fn max_f64(a: f64, b: f64) -> f64`
- `pub fn sqrt(x: f64) -> f64`, `pub fn pow(base: f64, exp: f64) -> f64`
- `pub fn floor(x: f64) -> f64`, `pub fn ceil(x: f64) -> f64`
- Math functions via extern C: `sqrt`, `pow`, `floor`, `ceil` from libm

### `std::string`
- `pub fn to_string(val: i64) -> String` (and other types)
- `pub fn parse_i64(s: String) -> i64` (or Result<i64, Error> if available)
- String utility methods (via impl on String, or as free functions)

### Core Utilities
- `pub fn assert(condition: bool, message: String)` — panic with message if false
- `pub fn panic(message: String) -> !` — abort with message
- `pub fn exit(code: i32) -> !` — exit process

### Prelude
- Auto-import common items: `print`, `println`, `assert`, `panic`, `Some`, `None`, `Ok`, `Err`
- Prelude is implicitly imported in every module

### File Structure
```
std/
  io.nudl
  math.nudl
  string.nudl
  prelude.nudl
```

## Acceptance Criteria
- `import std::io; io::println("hello");` works
- `import std::math; let x = math::sqrt(2.0);` works
- `assert(1 + 1 == 2, "math is broken");` works without import (prelude)
- `println("hello");` works without import (prelude)
- All math functions produce correct results
- String conversion functions work

## Technical Notes
- Stdlib is written in nudl using `extern` blocks for C functions
- Example `std/io.nudl`:
  ```
  extern {
    fn write(fd: i32, buf: RawPtr, count: i64) -> i64;
  }
  pub fn println(s: String) {
    // convert string to C call...
  }
  ```
- The stdlib path must be resolved by the compiler — environment variable or relative to binary
- Prelude auto-import: the pipeline inserts implicit `import std::prelude::*` at the start of each module
- Depends on: Task 21 (module system), Task 18 (string runtime), Task 04 (FFI types)
- Some stdlib functions may need to be compiler builtins if the language doesn't yet support the patterns needed to write them in nudl
