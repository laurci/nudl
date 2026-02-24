# nudl Implementation Status

## Legend
- [ ] Not started
- [~] In progress
- [x] Complete

## Pipeline Infrastructure
- [x] Lexer framework
- [x] Parser framework (recursive descent)
- [x] Type checker framework (two-pass)
- [x] SSA IR lowering framework
- [x] ARM64 codegen framework
- [x] Mach-O packer
- [ ] ELF packer
- [x] CLI (build, run, check commands)
- [x] VM interpreter
- [x] Diagnostic system with error codes
- [x] Source map and span tracking
- [x] String interning

## 1. Core Types
- [~] Integers — i32 ✓, i64 ✓, u64 ✓; i8/i16/u8/u16/u32 not yet (`tests/core-types/integers.nudl`)
- [ ] Floats — f32, f64 (`tests/core-types/floats.nudl`)
- [ ] Booleans (`tests/core-types/bool.nudl`)
- [ ] Characters (`tests/core-types/char.nudl`)
- [~] Strings (`tests/core-types/strings.nudl`)
- [ ] Format strings (`tests/core-types/format_strings.nudl`)
- [~] Unit type (`tests/core-types/unit.nudl`)
- [ ] Tuples (`tests/core-types/tuples.nudl`)
- [ ] Dynamic arrays T[] (`tests/core-types/dynamic_arrays.nudl`)
- [ ] Fixed-size arrays [T; N] (`tests/core-types/fixed_arrays.nudl`)
- [ ] Maps (`tests/core-types/maps.nudl`)
- [ ] Function types as values (`tests/core-types/function_types.nudl`)
- [ ] Never type (!)
- [ ] Range types
- [ ] FFI types — RawPtr ✓ in checker; MutRawPtr, CStr not yet

## 2. Variables & Bindings
- [~] Let bindings — immutable ✓, mut parsed but not enforced (`tests/variables/let_bindings.nudl`)
- [ ] Destructuring (`tests/variables/destructuring.nudl`)
- [~] Type annotations — basic types ✓, complex types not yet (`tests/variables/type_annotations.nudl`)
- [ ] Constants (`tests/variables/constants.nudl`)
- [ ] Comptime constants (`tests/variables/const_comptime.nudl`)
- [ ] Weak references (`tests/variables/weak_references.nudl`)

## 3. Operators
- [ ] Arithmetic (+, -, *, /, %) (`tests/operators/arithmetic.nudl`)
- [ ] Comparison (==, !=, <, >, <=, >=) (`tests/operators/comparison.nudl`)
- [ ] Logical (&&, ||, !) (`tests/operators/logical.nudl`)
- [ ] Bitwise (&, |, ^, <<, >>) (`tests/operators/bitwise.nudl`)
- [ ] Assignment (=, +=, -=, etc.) (`tests/operators/assignment.nudl`)
- [ ] Range (.., ..=) (`tests/operators/range.nudl`)
- [ ] Pipe (|>) (`tests/operators/pipe.nudl`)
- [ ] Type cast (as) (`tests/operators/type_cast.nudl`)
- [ ] Error propagation (?) (`tests/operators/error_propagation.nudl`)
- [ ] Precedence (`tests/operators/precedence.nudl`)

## 4. Control Flow
- [ ] If/else (`tests/control-flow/if_else.nudl`)
- [ ] If-let (`tests/control-flow/if_let.nudl`)
- [ ] Match (`tests/control-flow/match_basic.nudl`)
- [ ] For loops (`tests/control-flow/for_loops.nudl`)
- [ ] While loops (`tests/control-flow/while_loops.nudl`)
- [ ] Infinite loop (`tests/control-flow/loop_infinite.nudl`)
- [ ] Break/continue (`tests/control-flow/break_continue.nudl`)
- [ ] Labeled loops (`tests/control-flow/labeled_loops.nudl`)

## 5. Functions
- [x] Basic declarations & calls (`tests/functions/basic.nudl`)
- [ ] Named arguments (`tests/functions/named_arguments.nudl`)
- [ ] Argument shorthand (`tests/functions/argument_shorthand.nudl`)
- [ ] Default parameters (`tests/functions/default_params.nudl`)
- [ ] Optional parameters (`tests/functions/optional_params.nudl`)
- [ ] Closures (`tests/functions/closures.nudl`)
- [~] Methods — not yet (need structs + impl) (`tests/functions/methods.nudl`)
- [ ] Trailing lambdas (`tests/functions/trailing_lambda.nudl`)

## 6. User-Defined Types
- [ ] Unit structs (`tests/user-defined-types/struct_unit.nudl`)
- [ ] Tuple structs (`tests/user-defined-types/struct_tuple.nudl`)
- [ ] Named structs (`tests/user-defined-types/struct_named.nudl`)
- [ ] Struct spread (`tests/user-defined-types/struct_spread.nudl`)
- [ ] Unit enum variants (`tests/user-defined-types/enum_unit.nudl`)
- [ ] Struct enum variants (`tests/user-defined-types/enum_struct.nudl`)
- [ ] Data enum variants (`tests/user-defined-types/enum_data.nudl`)
- [ ] Type aliases (`tests/user-defined-types/type_aliases.nudl`)

## 7. Pattern Matching
- [ ] Literal patterns (`tests/pattern-matching/literal_patterns.nudl`)
- [ ] Tuple patterns (`tests/pattern-matching/tuple_patterns.nudl`)
- [ ] Struct patterns (`tests/pattern-matching/struct_patterns.nudl`)
- [ ] Enum patterns (`tests/pattern-matching/enum_patterns.nudl`)
- [ ] Nested patterns (`tests/pattern-matching/nested_patterns.nudl`)
- [ ] Or patterns (`tests/pattern-matching/or_patterns.nudl`)
- [ ] Binding patterns (`tests/pattern-matching/binding_patterns.nudl`)
- [ ] Wildcard patterns (`tests/pattern-matching/wildcard_patterns.nudl`)
- [ ] Range patterns (`tests/pattern-matching/range_patterns.nudl`)
- [ ] Guard clauses (`tests/pattern-matching/guard_clauses.nudl`)
- [ ] Exhaustiveness checking (`tests/pattern-matching/exhaustiveness.nudl`)

## 8. Generics
- [ ] Generic functions (`tests/generics/generic_functions.nudl`)
- [ ] Generic structs (`tests/generics/generic_structs.nudl`)
- [ ] Generic enums (`tests/generics/generic_enums.nudl`)
- [ ] Bounds (`tests/generics/bounds.nudl`)
- [ ] Where clauses (`tests/generics/where_clauses.nudl`)
- [ ] Turbofish syntax (`tests/generics/turbofish.nudl`)
- [ ] Monomorphization (`tests/generics/monomorphization.nudl`)

## 9. Interfaces
- [ ] Declaration (`tests/interfaces/declaration.nudl`)
- [ ] Implementation (`tests/interfaces/implementation.nudl`)
- [ ] Inherent methods (`tests/interfaces/inherent_methods.nudl`)
- [ ] Generic interfaces (`tests/interfaces/generic_interfaces.nudl`)
- [ ] Dynamic dispatch (dyn) (`tests/interfaces/dynamic_dispatch.nudl`)
- [ ] Method resolution (`tests/interfaces/method_resolution.nudl`)
- [ ] Operator overloading (`tests/interfaces/operator_overloading.nudl`)

## 10. Error Handling
- [ ] Option type (`tests/error-handling/option.nudl`)
- [ ] Result type (`tests/error-handling/result.nudl`)
- [ ] Panic (`tests/error-handling/panic.nudl`)
- [ ] ? operator (`tests/error-handling/question_mark.nudl`)

## 11. Memory Management
- [ ] ARC sharing (`tests/memory-management/arc_sharing.nudl`)
- [ ] ARC deallocation (`tests/memory-management/arc_deallocation.nudl`)
- [ ] Value type copy (`tests/memory-management/value_type_copy.nudl`)
- [~] Mutability — parsed, not enforced (`tests/memory-management/mutability.nudl`)
- [ ] Defer (`tests/memory-management/defer.nudl`)
- [ ] Drop interface (`tests/memory-management/drop_interface.nudl`)
- [ ] Clone interface (`tests/memory-management/clone_interface.nudl`)
- [ ] Weak references (`tests/memory-management/weak_upgrade.nudl`)
- [ ] Aliased mutation (`tests/memory-management/aliased_mutation.nudl`)

## 12. Modules
- [ ] Basic imports (`tests/modules/basic-import/`)
- [ ] Grouped imports (`tests/modules/grouped-import/`)
- [ ] Aliased imports (`tests/modules/aliased-import/`)
- [ ] Glob imports (`tests/modules/glob-import/`)
- [ ] Module paths (`tests/modules/module-paths/`)
- [ ] Visibility (`tests/modules/visibility/`)

## 13. Async & Concurrency
- [ ] Async functions (`tests/async/async_fn.nudl`)
- [ ] Async blocks (`tests/async/async_blocks.nudl`)
- [ ] Postfix await (`tests/async/postfix_await.nudl`)
- [ ] Prefix await (`tests/async/prefix_await.nudl`)
- [ ] Task.spawn (`tests/async/task_spawn.nudl`)
- [ ] Task groups (`tests/async/task_groups.nudl`)
- [ ] Actors (`tests/async/actors.nudl`)
- [ ] Cancellation (`tests/async/cancellation.nudl`)

## 14. Comptime & Metaprogramming
- [ ] Comptime blocks (`tests/comptime/comptime_block.nudl`)
- [ ] Comptime functions (`tests/comptime/comptime_function.nudl`)
- [ ] Comptime parameters (`tests/comptime/comptime_params.nudl`)
- [ ] Quote/splice (`tests/comptime/quote_splice.nudl`)
- [ ] Code generation (`tests/comptime/code_generation.nudl`)
- [ ] Attributes (`tests/comptime/attributes.nudl`)
- [ ] Type introspection (`tests/comptime/type_introspection.nudl`)
- [ ] Types as values (`tests/comptime/types_as_values.nudl`)
- [ ] AST inspection (`tests/comptime/ast_inspection.nudl`)
- [ ] Module introspection (`tests/comptime/module_introspection.nudl`)

## 15. Misc
- [x] Comments (line + nested block) (`tests/misc/comments.nudl`)
- [ ] Block expressions as values (`tests/misc/block_expressions.nudl`)
- [ ] Method chaining (`tests/misc/method_chaining.nudl`)
- [ ] Spread operator (`tests/misc/spread_operator.nudl`)

## Features Without Dedicated Tests
- [ ] FFI extern blocks (partially working, no dedicated test)
- [ ] String interpolation nesting
- [ ] Derive macros
- [ ] Build scripts (build.nudl)
- [ ] Package/dependency management (nudl.toml)
- [ ] Standard library (std::math, std::io, iterators)
- [ ] Const at module level
- [ ] Extern statics
- [ ] Callbacks (#[extern_callable])
