# nudl Implementation Status

## Legend
- [ ] Not started
- [~] Partially implemented (see notes)
- [x] Complete (works end-to-end: source → type check → IR → native code)

## Pipeline Infrastructure
- [x] Lexer framework
- [x] Parser framework (recursive descent + Pratt precedence)
- [x] Type checker framework (two-pass: collect declarations → check bodies)
- [x] SSA IR lowering framework
- [x] LLVM backend via Inkwell (replaced ARM64 codegen + Mach-O/ELF packers)
- [x] CLI (build, run, check, fmt commands + --dump-ast, --dump-ir, --dump-llvm-ir, --dump-asm)
- [x] VM interpreter (register-based, step-limited, for comptime eval) — supports closures (ClosureCreate/ClosureCall), dynamic arrays (alloc/push/pop/get/set/len), and maps (alloc/insert/get/contains/len)
- [x] Diagnostic system with error codes and severity levels
- [x] Source map and span tracking
- [x] String interning
- [x] LSP server (diagnostics on document change)
- [x] Debug symbols (DWARF) generation
- [~] ARC runtime (`runtime/nudl_rt.c`) — compiled at build time, linked into output binaries; inline LLVM retain/release fast paths; compiler now emits Retain/Release for struct types; runtime extended with dynamic array (alloc/push/pop/get/set/len), map (hash table alloc/insert/get/contains/remove/len), and closure environment allocation

## 1. Core Types
- [x] Integers — all types (i8, i16, i32, i64, u8, u16, u32, u64) in type checker; IR constants for i32, i64, u64; others coerce from unsuffixed literals (`tests/core-types/integers.nudl`)
- [x] Floats — f64 in type checker + IR; f32 in type checker + IR + codegen (`tests/core-types/floats.nudl`)
- [x] Booleans (`tests/core-types/bool.nudl`)
- [x] Characters (`tests/core-types/char.nudl`)
- [x] Strings — reference type with (ptr, len) pair expansion; string indexing `s[i]` → char via inline LLVM IR with bounds check (`tests/core-types/strings.nudl`)
- [x] Template strings — lexer/parser handle backtick interpolation with brace nesting; lowered via __str_concat and __*_to_str builtins (`tests/core-types/format_strings.nudl`)
- [x] Unit type (`tests/core-types/unit.nudl`)
- [x] Tuples — tuple types `(T1, T2)`, tuple literals, `.0`/`.1` element access, tuples as function params/returns, let destructuring (`tests/core-types/tuples_basic.nudl`)
- [x] Dynamic arrays T[] — TypeKind::DynamicArray in type system, parsed as `T[]`, type-checked; runtime C implementation (alloc, push with auto-grow, pop, get, set, len); SSA instructions DynArrayAlloc/Push/Pop/Len/Get/Set; LLVM codegen calls runtime; for-each iteration works (`tests/core-types/dynamic_arrays.nudl`)
- [x] Fixed-size arrays [T; N] — array literals, index access, mutable index assignment, array repeat `[0; 5]`, type annotations (`tests/core-types/fixed_arrays_basic.nudl`)
- [x] Maps — TypeKind::Map in type system, parsed as `Map<K, V>`, type-checked; runtime C hash map implementation (open-addressing, linear probing, auto-grow at 70% load); Map::new(), insert, get, contains_key, len; SSA instructions MapAlloc/Insert/Get/Len/Contains; LLVM codegen calls runtime (`tests/core-types/maps.nudl`)
- [x] Function types as values — closures are first-class Function values with capture environments; closures can be called, passed, and stored (`tests/core-types/function_types.nudl`)
- [x] Never type (!) — TypeKind::Never, pre-interned, recognized in type checker
- [~] Range types — `..` and `..=` operators parsed/lowered for use in for-in loops; no standalone Range struct yet
- [x] FFI types — RawPtr, MutRawPtr, CStr all in type checker + codegen; cast support between pointer types

## 2. Variables & Bindings
- [x] Let bindings — immutable and mutable, with mutability enforcement in type checker (`tests/variables/let_bindings.nudl`)
- [x] Destructuring — let destructuring for tuples `let (a, b) = expr` and structs `let Foo { x, y } = expr` (`tests/variables/destructuring.nudl`)
- [x] Type annotations — primitive types and string; complex types not yet (`tests/variables/type_annotations.nudl`)
- [x] Constants — `const` declarations (parsed, type-checked, lowered as immutable let) (`tests/variables/constants.nudl`)
- [ ] Comptime constants (`tests/variables/const_comptime.nudl`)
- [ ] Weak references (`tests/variables/weak_references.nudl`)

## 3. Operators
- [x] Arithmetic (+, -, *, /, %, unary -) (`tests/operators/arithmetic.nudl`)
- [x] Comparison (==, !=, <, >, <=, >=) (`tests/operators/comparison.nudl`)
- [x] Logical (&&, ||, !) — with short-circuit evaluation (`tests/operators/logical.nudl`)
- [x] Bitwise — all ops (&, |, ^, ~, <<, >>) parsed, type-checked, lowered, codegen'd (`tests/operators/bitwise.nudl`)
- [x] Assignment (=, +=, -=, *=, /=, %=, <<=, >>=, &=, |=, ^=) — all compound assignments including bitwise (`tests/operators/assignment.nudl`)
- [x] Range (.., ..=) — parsed as infix operators, used in for-in loops via while-loop desugaring (`tests/operators/range.nudl`)
- [x] Pipe (|>) — parsed and desugared to function calls at parse time (`tests/operators/pipe.nudl`)
- [x] Type cast (as) — postfix `as Type` with numeric↔numeric, bool→int, char↔any integer, any integer↔char, ptr casts (`tests/operators/type_cast.nudl`)
- [x] Error propagation (?) — parsed as postfix operator; type checker extracts inner type from Option<T>/Result<T,E> including monomorphized names (Option$i32, Result$i32$string, etc.); lowered to tag check + branch with early return (None for Option, Err(e) for Result) (`tests/operators/error_propagation.nudl`)
- [x] Precedence — Pratt climbing with correct binding power levels (`tests/operators/precedence.nudl`)

## 4. Control Flow
- [x] If/else — with tail expression semantics, if-else-if chains (`tests/control-flow/if_else.nudl`)
- [x] If-let — `if let Pattern = expr { ... } else { ... }` with enum variant destructuring (`tests/control-flow/if_let.nudl`)
- [x] Match — `match expr { pattern => body }` with wildcard, binding, literal, and enum patterns; lowered to tag-compare + branch chains (`tests/control-flow/match_basic.nudl`)
- [x] For loops — `for x in 0..n`, `for x in 0..=n`, `for x in array`; desugared to while loops at IR lowering (`tests/control-flow/for_loops_basic.nudl`)
- [x] While loops (`tests/control-flow/while_loops.nudl`)
- [x] Infinite loop (`tests/control-flow/loop_infinite.nudl`)
- [x] Break/continue (`tests/control-flow/break_continue.nudl`)
- [x] Labeled loops — `'label: while/loop`, `break 'label`, `continue 'label` (`tests/control-flow/labeled_loops.nudl`)

## 5. Functions
- [x] Basic declarations & calls (`tests/functions/basic.nudl`)
- [x] Named arguments — parser sets `CallArg.name`, checker resolves by parameter name, lowerer reorders to positional (`tests/functions/named_arguments.nudl`)
- [x] Argument shorthand — struct field shorthand `S { x, y }` desugared at parse time; function call shorthand works positionally (`tests/functions/argument_shorthand.nudl`)
- [x] Default parameters — `Param.default_value` in AST, checker validates required vs optional, lowerer fills defaults at call sites (`tests/functions/default_params.nudl`)
- [ ] Optional parameters (`tests/functions/optional_params.nudl`)
- [x] Closures — `|params| body` and `|params| -> T { body }` syntax; free variable capture analysis; capture environment heap-allocated as ARC object; closure thunks lowered as separate functions with env as first param; indirect call via function pointer; SSA instructions ClosureCreate/ClosureCall; LLVM codegen with ptrtoint/inttoptr for function pointers (`tests/functions/closures.nudl`)
- [x] Methods — `impl` blocks parsed, methods registered with mangled names (`Type__method`), `self`/`mut self` params, method calls `obj.method()` and static calls `Type::method()` (`tests/functions/methods.nudl`)
- [x] Trailing lambdas — `func(args) |x| body`, `func(args) || body`, `func(args) { body }` (implicit `it` param); trailing closure appended as last argument at parse time; inhibited inside if/while/for conditions to prevent block stealing (`tests/functions/trailing_lambda.nudl`)
- [x] Function type syntax — `|T1, T2| -> R` and `|| -> R` in type position; parsed as `TypeExpr::FnType`, resolved to `TypeKind::Function` in checker and lowerer

## 6. User-Defined Types
- [x] Unit structs — `struct Foo;` parsed, type-checked, constructed via identifier (Alloc + Retain) (`tests/user-defined-types/struct_unit.nudl`)
- [x] Tuple structs — `struct Foo(T1, T2);` parsed with positional fields ("0", "1", ...), constructed via call syntax (Alloc + Store fields + Retain) (`tests/user-defined-types/struct_tuple.nudl`)
- [~] Named structs — declaration, construction, field access, field assignment, ARC caller-retain/callee-release, scope-exit release, impl blocks with methods, let destructuring, generics with monomorphization (`tests/user-defined-types/struct_simple.nudl`); no spread yet
- [ ] Struct spread (`tests/user-defined-types/struct_spread.nudl`)
- [x] Unit enum variants — `enum Color { Red, Green, Blue }` parsed, type-checked, lowered as tag-only heap objects (`tests/user-defined-types/enum_unit.nudl`)
- [x] Struct enum variants — `enum Shape { Circle { radius: i32 } }` parsed and type-checked (`tests/user-defined-types/enum_struct.nudl`)
- [x] Data enum variants — `enum Shape { Circle(i32), Rectangle(i32, i32) }` parsed, type-checked, lowered with tag + fields; impl blocks on enums work (`tests/user-defined-types/enum_data.nudl`)
- [x] Type aliases — `type Name = ExistingType;` parsed, resolved during type checking, used transparently in all type positions (`tests/user-defined-types/type_aliases.nudl`)

## 7. Pattern Matching
- [x] Literal patterns — integer, bool, string literals in match arms (`tests/pattern-matching/literal_patterns.nudl`)
- [x] Tuple patterns — `(a, b, c)` patterns in match arms; lowered via TupleLoad to extract elements and bind to variables (`tests/pattern-matching/tuple_patterns.nudl`)
- [x] Struct patterns — `Struct { field, .. }` patterns in let destructuring and match arms; parse_pattern() now checks for uppercase+LBrace to dispatch to parse_struct_pattern(); lowered via Load to extract fields by index (`tests/pattern-matching/struct_patterns.nudl`)
- [x] Enum patterns — `Enum::Variant(binding)` with tag comparison and field extraction; unqualified variant patterns `Some(v)` / `None` supported (`tests/pattern-matching/enum_patterns.nudl`)
- [ ] Nested patterns (`tests/pattern-matching/nested_patterns.nudl`)
- [ ] Or patterns (`tests/pattern-matching/or_patterns.nudl`)
- [x] Binding patterns — `name` binds the scrutinee to a variable in the arm body (`tests/pattern-matching/binding_patterns.nudl`)
- [x] Wildcard patterns — `_` matches anything (`tests/pattern-matching/wildcard_patterns.nudl`)
- [ ] Range patterns (`tests/pattern-matching/range_patterns.nudl`)
- [x] Guard clauses — `pattern if condition => body` in match arms; guard evaluated after pattern match, branches to next arm on failure (`tests/pattern-matching/guard_clauses.nudl`)
- [ ] Exhaustiveness checking (`tests/pattern-matching/exhaustiveness.nudl`)

## 8. Generics
- [x] Generic functions — type parameters `<T>` parsed; checker-phase monomorphization with type inference from call arguments; mangled names `base$type1$type2`; works end-to-end (`tests/generics/generic_fn.nudl`)
- [x] Generic structs — type parameters parsed; monomorphized when struct literal used with concrete field types; fields resolved via substitution; impl methods instantiated per monomorphization (`tests/generics/generic_struct.nudl`)
- [x] Generic enums — type parameters parsed; monomorphized when variant constructor called with concrete args; variants resolved via substitution (`tests/generics/generic_enum.nudl`)
- [x] Generic impl blocks — `impl Wrapper<T>` methods stored as templates; instantiated per struct/enum monomorphization with correct self type and substituted params/return types (`tests/generics/generic_impl.nudl`)
- [x] Monomorphization — checker-phase: generic defs stored as templates, concrete copies created on use with mangled names; resolution maps bridge checker to lowerer; pending mono body loop handles recursive monomorphization (`tests/generics/generic_combined.nudl`)
- [~] Bounds — `<T: Bound>` syntax parsed on type parameters; bounds stored; shallow body checking validates binary ops against required bounds (Add, Eq, Ord); `BoundNotSatisfied` diagnostic emitted when missing; full enforcement deferred to monomorphization (`tests/generics/bounds.nudl`)
- [x] Generic body shallow checking — generic function and impl method bodies are shallow-checked at definition time using TypeVar placeholders; catches undefined variables, undefined functions, return type mismatches, and other obvious errors without requiring monomorphization
- [ ] Where clauses (`tests/generics/where_clauses.nudl`)
- [ ] Turbofish syntax — `::<T>` explicit type args not yet parsed (`tests/generics/turbofish.nudl`)

## 9. Interfaces
- [x] Declaration — `interface Name { fn method(self) -> T; }` parsed and type-checked; interface types registered in type system (`tests/interfaces/declaration.nudl`)
- [~] Implementation — `impl Interface for Type { ... }` parsed and type-checked; interface impls tracked; no vtable dispatch yet (`tests/interfaces/implementation.nudl`)
- [x] Inherent methods — impl blocks without interface work for both structs and enums (`tests/interfaces/inherent_methods.nudl`)
- [ ] Generic interfaces (`tests/interfaces/generic_interfaces.nudl`)
- [~] Dynamic dispatch (dyn) — `dyn Interface` type parsed and in type system; no vtable codegen yet (`tests/interfaces/dynamic_dispatch.nudl`)
- [x] Method resolution — methods resolved via mangled names `Type__method` for both structs and enums (`tests/interfaces/method_resolution.nudl`)
- [x] Operator overloading — binary operators (+, -, *, /, %, ==, !=, <, <=, >, >=) dispatch to interface methods (add, sub, mul, div, rem, eq, ne, lt, le, gt, ge) via mangled `Type__method` lookup; prelude defines Add, Sub, Mul, Div, Rem, Eq, Ord, Neg interfaces (`tests/interfaces/operator_overloading.nudl`)

## 10. Error Handling
- [x] Option type — `Option<T>` enum (Some/None) defined in nudl-std/prelude.nudl with methods: is_some, is_none, unwrap, unwrap_or; works with generics and ? operator (`tests/error-handling/option.nudl`)
- [x] Result type — `Result<T, E>` enum (Ok/Err) defined in nudl-std/prelude.nudl with methods: is_ok, is_err, unwrap, unwrap_or; return type inference for multi-type-param enums (`tests/error-handling/result.nudl`)
- [x] Panic — `panic(string)` registered as builtin, type-checked, lowered to builtin call (`tests/error-handling/panic.nudl`)
- [x] ? operator — parsed as postfix operator; type checker extracts T from Option<T> (Some variant) and Result<T,E> (Ok variant); lowered to tag check + branch with early return of None/Err (`tests/error-handling/question_mark.nudl`)

## 11. Memory Management
- [~] ARC runtime — C runtime (alloc, release_slow, overflow_abort, weak ops) + inline LLVM retain/release; SSA instructions (Alloc, Load, Store, Retain, Release) in IR + backend + VM; compiler emits Retain/Release for struct and enum types (caller-retain, callee-release, scope-exit release)
- [ ] ARC sharing (`tests/memory-management/arc_sharing.nudl`)
- [ ] ARC deallocation (`tests/memory-management/arc_deallocation.nudl`)
- [ ] Value type copy (`tests/memory-management/value_type_copy.nudl`)
- [x] Mutability enforcement — type checker rejects assignment to immutable bindings (`tests/memory-management/mutability.nudl`)
- [x] Defer — `defer { ... }` parsed, type-checked, lowered (deferred blocks emitted LIFO before function return) (`tests/memory-management/defer.nudl`)
- [ ] Drop interface (`tests/memory-management/drop_interface.nudl`)
- [ ] Clone interface (`tests/memory-management/clone_interface.nudl`)
- [ ] Weak references (`tests/memory-management/weak_upgrade.nudl`)
- [ ] Aliased mutation (`tests/memory-management/aliased_mutation.nudl`)

## 12. Modules
- [x] Basic imports — `import std::io;` parsed, resolved relative to source dir and nudl-std/ (`tests/modules/basic-import/`)
- [x] Grouped imports — `import std::io::{print, println};` parsed (`tests/modules/grouped-import/`)
- [x] Aliased imports — `import std::io as io_lib;` parsed (`tests/modules/aliased-import/`)
- [x] Glob imports — `import std::io::*;` parsed (`tests/modules/glob-import/`)
- [x] Module paths — `::` path resolution, nudl-std/ search, workspace root detection (`tests/modules/module-paths/`)
- [x] Prelude auto-import — nudl-std/prelude.nudl is automatically imported into all user programs (unless the file is the prelude itself); provides Option<T>, Result<T,E>, operator interfaces, core interfaces, print/println, collection utils, min/max/clamp
- [ ] Visibility — `pub` keyword parsed but not enforced at import boundaries (`tests/modules/visibility/`)

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
- [x] Block expressions as values — tail expression semantics (`tests/misc/block_expressions.nudl`)
- [ ] Method chaining (`tests/misc/method_chaining.nudl`)
- [ ] Spread operator (`tests/misc/spread_operator.nudl`)

## Features Without Dedicated Tests
- [x] FFI extern blocks — extern function declarations and calls work end-to-end
- [x] String interpolation nesting — lexer handles brace-depth tracking, lowered via __str_concat chain
- [ ] Derive macros
- [ ] Build scripts (build.nudl)
- [ ] Package/dependency management (nudl.toml)
- [x] Standard library — nudl-std/ with auto-imported prelude: generic Option<T>/Result<T,E> with methods, print/println via extern write, operator interfaces (Add, Sub, Mul, Div, Rem, Eq, Ord, Neg), core interfaces (Printable, Drop, Clone), generic collection functions (each, map, filter, fold, find, any, all, enumerate, reverse), min/max/clamp; string module with builtins: substr, indexof, trim, contains, starts_with, ends_with, to_upper, to_lower, replace, repeat, split, join; math module (PI, E, abs, pow, gcd, lcm); ffi module (cstr, cstr_len); io module with file operations (open, close_fd, read, file_write, read_file, write_file, append_file)
- [x] Null-terminated strings — all heap strings and string literal globals are null-terminated for C FFI compatibility; length field unchanged
- [x] Extern string returns — extern functions returning string now return ptr (ARC heap string), extracted via extract_heap_string in codegen
- [ ] Const at module level
- [ ] Extern statics
- [ ] Callbacks (#[extern_callable])
