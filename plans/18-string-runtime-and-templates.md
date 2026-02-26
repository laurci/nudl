# Task 18: String Runtime and Template Strings

## Goal
Implement a C runtime for string operations (concatenation, to_string conversions) with ARC management, and lower template strings (backtick interpolation) to string concatenation.

## Requirements

### String Runtime (`runtime/nudl_rt.c`)
- `__nudl_string_alloc(ptr: *const u8, len: i64) -> StringPtr` — allocate ARC'd string from data
- `__nudl_string_concat(a: StringPtr, b: StringPtr) -> StringPtr` — concatenate two strings
- `__nudl_string_len(s: StringPtr) -> i64` — get length
- `__nudl_string_eq(a: StringPtr, b: StringPtr) -> bool` — string equality
- Type conversion functions:
  - `__nudl_i64_to_string(val: i64) -> StringPtr`
  - `__nudl_f64_to_string(val: f64) -> StringPtr`
  - `__nudl_bool_to_string(val: bool) -> StringPtr`
  - `__nudl_char_to_string(val: char) -> StringPtr`

### String ARC
- String object layout: `{ ref_count: i64, length: i64, data: [u8] }` (data inline after header)
- Retain/Release follow the standard ARC pattern
- String literals: compile-time static strings (no ARC needed — immortal refcount)
- Concatenation creates a new string (immutable strings)

### Template String Lowering
- Backtick strings: `` `Hello, ${name}! You are ${age} years old.` ``
- Lower to string concatenation: `"Hello, " + to_string(name) + "! You are " + to_string(age) + " years old."`
- Parser already handles backtick tokenization with interpolation brace nesting
- Need to lower the template AST nodes to concatenation + to_string calls

### String Concatenation Operator
- `string + string` → calls `__nudl_string_concat`
- `string + non_string` → type error (require explicit to_string)
- Or: auto-convert in templates only

### Type Checking
- Template expressions can be any type that has a to_string conversion
- Validate interpolated expressions exist and have known types

## Acceptance Criteria
- `tests/core-types/strings.nudl` works with concat and comparisons
- `tests/core-types/format_strings.nudl` compiles and runs
- Template: `` `1 + 2 = ${1 + 2}` `` → `"1 + 2 = 3"`
- String concat: `"hello" + " " + "world"` → `"hello world"`
- Nested interpolation: `` `outer ${`inner ${x}`}` ``
- ARC: strings are properly reference counted (no leaks)
- to_string conversions for i32, i64, f64, bool, char

## Technical Notes
- Current string representation is `(ptr, len)` pair expanded in function signatures
- Need to decide: keep this ABI or switch to single pointer to string object?
- If keeping (ptr, len): runtime functions take/return ptr+len pairs
- If switching to single ptr: simpler ABI but changes existing calling convention
- Template lowering happens during AST→IR lowering or as an AST desugaring pass
- The lexer/parser already handle `` `text ${expr} text` `` — check `TemplateString` AST nodes
- `snprintf` or similar for numeric→string conversion in the C runtime
- Depends on: existing string infrastructure works; this extends it
