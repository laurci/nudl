# Task 22: String Runtime, Template Strings, and Bytes Type

## Goal
Implement a C runtime for string operations (concatenation, to_string conversions) with ARC management, lower template strings (backtick interpolation) to string concatenation, and add a first-class `bytes` type with the same infrastructure. Switch string ABI from `(ptr, len)` pairs to single pointer to ARC object.

## Requirements

### ABI Change: Single Pointer to ARC Object
- **Decision**: Switch from `(ptr, len)` pair expansion to single pointer
- All string/bytes values are pointers to: `{ NudlArcHeader(16 bytes), length: i64, data: [u8] }`
- String literals become immortal globals with `strong_count = UINT32_MAX`
- Remove `RegStringInfo` enum and `(ptr, len)` special-casing from codegen
- Simplify `ParamLayout`, `build_llvm_param_types`, call marshaling, parameter setup
- `StringPtr`/`StringLen` instructions compute from object layout (offsets 24 and 16)

### String Runtime (`runtime/nudl_rt.c`)
- `__nudl_string_concat(a, b) -> ptr` — allocate new ARC string, copy both
- `__nudl_string_eq(a, b) -> bool` — compare length + memcmp
- `__nudl_string_cmp(a, b) -> i64` — lexicographic compare (for `<`, `>`)
- `__nudl_string_len(s) -> i64` — read length field
- `__nudl_string_data(s) -> ptr` — return pointer to data (offset 24)
- Type conversion functions:
  - `__nudl_i64_to_string(val: i64) -> ptr`
  - `__nudl_f64_to_string(val: f64) -> ptr`
  - `__nudl_bool_to_string(val: bool) -> ptr`
  - `__nudl_char_to_string(val: char) -> ptr`

### Bytes Runtime (`runtime/nudl_rt.c`)
- `__nudl_bytes_concat(a, b) -> ptr` — same as string concat
- `__nudl_bytes_eq(a, b) -> bool` — same as string eq
- `__nudl_bytes_len(b) -> i64` — read length field
- `__nudl_bytes_index(b, idx) -> u8` — bounds-checked byte access

### Bytes Type
- New `TypeKind::Bytes` in type system (pre-interned at index 19)
- Reference type, ARC-managed, same object layout as string
- Immutable (like strings)
- Literal syntax: `b"hello\x00\xff"` with hex escapes
- `bytes + bytes` → concat, `bytes == bytes` → equality

### String ARC
- Retain/Release follow the standard ARC pattern
- Immortal refcount (`UINT32_MAX`): retain/release become no-ops
- Strings bypass ARC currently — fix by replacing `is_struct()` with `is_reference_type()` in lowerer

### Template String Lowering
- Backtick strings: `` `Hello, {name}! You are {age} years old.` ``
- Lower to string concatenation: `"Hello, " + to_string(name) + "! You are " + to_string(age) + " years old."`
- Parser already handles backtick tokenization with interpolation brace nesting
- Need to lower the template AST nodes to concatenation + to_string calls

### String/Bytes Concatenation Operator
- `string + string` → calls `__nudl_string_concat`
- `bytes + bytes` → calls `__nudl_bytes_concat`
- `string + non_string` → type error (require explicit to_string)
- Auto-convert in templates only (via `ToString` IR instruction)

### Type Checking
- Template expressions can be any type that has a to_string conversion
- Validate interpolated expressions exist and have known types
- `+` operator extended for string and bytes operands
- `==`/`!=` extended for string and bytes
- `<`/`<=`/`>`/`>=` extended for string (lexicographic)
- `Literal::Bytes(...)` → type is `bytes`

### New IR Instructions
- `StringConcat(dst, a, b)`, `StringEq(dst, a, b)`, `StringCmp(dst, a, b)`
- `ToString(dst, src, TypeId)` — dispatch to appropriate runtime function
- `BytesConcat(dst, a, b)`, `BytesEq(dst, a, b)`, `BytesIndex(dst, src, idx)`
- `ConstValue::BytesLiteral(u32)` — index into `Program::bytes_constants`

## Acceptance Criteria
- `tests/core-types/strings.nudl` works with concat and comparisons
- `tests/core-types/format_strings.nudl` compiles and runs
- Template: `` `1 + 2 = {1 + 2}` `` → `"1 + 2 = 3"`
- String concat: `"hello" + " " + "world"` → `"hello world"`
- Nested interpolation: `` `outer {`inner {x}`}` ``
- ARC: strings are properly reference counted (no leaks via immortal refcount)
- to_string conversions for i32, i64, f64, bool, char
- Bytes: `b"hello" + b"\x00"` concatenation works
- Bytes: `b"abc" == b"abc"` → true
- Existing tests pass after ABI change

## Technical Notes
- Current string representation is `(ptr, len)` pair — switching to single pointer
- Immortal refcount sentinel: `UINT32_MAX` — modify retain to no-op, release to skip
- Template lowering: build list of string segments, left-fold with `StringConcat`
- `infer_expr_type` in lowerer needs extension for `TemplateString`, `Bytes`, string `+`
- Replace all `is_struct()` checks with `is_reference_type()` in lowerer for ARC correctness
- `snprintf` or similar for numeric→string conversion in the C runtime
- VM: new instructions can return `VmError` for MVP (comptime string ops deferred)
- Depends on: existing string infrastructure works; this extends it
