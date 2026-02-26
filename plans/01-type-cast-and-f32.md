# Task 01: Type Cast (`as`) Operator + f32 IR/Codegen Support

## Goal
Implement the `as` operator for numeric type conversions and complete f32 support through IR and LLVM codegen, so users can write `let x: f32 = 3.14 as f32` and perform all numeric casts.

## Requirements

### `as` Operator
- Parse `expr as Type` as a postfix expression with appropriate precedence (higher than comparison, lower than unary)
- Support casts between all numeric types: i8, i16, i32, i64, u8, u16, u32, u64, f32, f64
- Support `bool as integer` (true→1, false→0)
- Support `char as u32` and `u32 as char`
- Reject non-numeric casts at type-check time with a clear diagnostic

### f32 Support
- Add `ConstValue::F32(f32)` to SSA IR (f64 already exists)
- LLVM codegen: emit `f32` constants and instructions (fadd, fsub, fmul, fdiv for f32)
- Ensure f32 literals work: `let x: f32 = 1.5` (currently only f64 literals exist)

### IR
- Add `Instruction::Cast { dst, src, target_type }` to SSA IR
- Lower `as` expressions to Cast instructions
- LLVM codegen: emit appropriate LLVM cast instructions:
  - int→int: trunc/sext/zext based on signedness and size
  - float→float: fptrunc/fpext
  - int→float: sitofp/uitofp
  - float→int: fptosi/fptoui

## Acceptance Criteria
- `tests/operators/type_cast.nudl` compiles and runs correctly
- `tests/core-types/floats.nudl` works with both f32 and f64
- Casting between all numeric types produces correct values
- Invalid casts (e.g., `string as i32`) produce a type error

## Technical Notes
- Parser: add `as` to the Pratt parser as a postfix operator in `parse_expr` (similar to how field access works but consuming a type instead of identifier)
- Type checker (`nudl-bc/src/checker.rs`): validate cast pairs, produce typed Cast node
- Lowerer (`nudl-bc/src/lower.rs`): emit `Instruction::Cast`
- Codegen (`nudl-backend-llvm/src/codegen.rs`): map Cast to LLVM builder methods (`build_int_cast`, `build_float_cast`, `build_int_to_float`, etc.)
- The parser likely needs a `parse_type` call after the `as` keyword
