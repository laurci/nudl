# Task 02: Bitwise Operators

## Goal
Complete bitwise operator support: `&`, `|`, `^`, `~` (bitwise NOT) plus compound assignments `&=`, `|=`, `^=`. Shift operators `<<`, `>>` and their assignments `<<=`, `>>=` already work.

## Requirements

### Parsing
- Parse `&`, `|`, `^` as binary infix operators with correct precedence:
  - `&` between equality and `|`
  - `^` between `&` and `|`
  - `|` below `^`, above logical `&&`
- Parse `~` as a unary prefix operator (same precedence as `!` and unary `-`)
- Tokens already exist; they just need to be wired into the expression parser

### Type Checking
- All bitwise ops require integer operands (any int type)
- Both operands must be the same type (no implicit widening)
- Result type matches operand type
- `~` requires an integer operand, result is same type

### IR
- Add instructions: `BitAnd`, `BitOr`, `BitXor`, `BitNot`
- Lower compound assignments (`&=`, `|=`, `^=`) to load + op + store (same pattern as existing `<<=`, `>>=`)

### Codegen
- LLVM: `build_and`, `build_or`, `build_xor`, `build_not` (all straightforward)

## Acceptance Criteria
- `tests/operators/bitwise.nudl` compiles and runs correctly
- All bitwise operators work on all integer types
- Compound assignments work correctly
- Type errors for non-integer operands

## Technical Notes
- The tokens `Amp`, `Pipe`, `Caret`, `Tilde` already exist in the lexer
- `Amp` is also used for reference types — context should disambiguate (binary position vs type position)
- `Pipe` is also used for closure params — context should disambiguate
- Compound assignment tokens `AmpEq`, `PipeEq`, `CaretEq` already exist
- STATUS.md says `<<`, `>>` are implemented and `<<=`, `>>=` assignments work, so follow the same pattern
