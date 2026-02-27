# nudl MVP Task Plan

## Goal
Complete all of STATUS.md §1-5 plus supporting infrastructure (enums, pattern matching, generics, interfaces, etc.), a basic module system, and a stdlib written in nudl using extern C. End result: a language supporting string interpolation, stdlib calls, ARC memory management, C FFI, closures, pattern matching, for-loops, methods, generics, interfaces, arrays, maps, enums, and more.

## Status Legend
- [ ] Not started
- [~] In progress
- [x] Complete

## Execution Order & Dependencies

### Phase 1: Foundation (independent, no cross-deps) — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 01 | [x] | [01-type-cast-and-f32.md](01-type-cast-and-f32.md) | `as` operator + f32 IR/codegen | — |
| 02 | [x] | [02-bitwise-operators.md](02-bitwise-operators.md) | `&`, `|`, `^`, `~` + compound assignments | — |
| 03 | [x] | [03-constants-labels-never.md](03-constants-labels-never.md) | `const`, labeled loops, `!` type | — |
| 04 | [x] | [04-ffi-types.md](04-ffi-types.md) | `MutRawPtr`, `CStr` types | — |
| 05 | [x] | [05-pipe-operator.md](05-pipe-operator.md) | `|>` pipe operator | — |

> **All 5 tasks in Phase 1 can be done in parallel.**

### Phase 2: Compound Types — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 06 | [x] | [06-tuple-types.md](06-tuple-types.md) | Tuple types, literals, `.0` access | — |
| 07 | [x] | [07-fixed-arrays.md](07-fixed-arrays.md) | `[T; N]` type, index access | — |
| 08 | [x] | [08-range-and-for-loops.md](08-range-and-for-loops.md) | Range types, for-in loops | 07 (array iteration) |

> **06 and 07 can be done in parallel. 08 depends on 07 for array iteration (range iteration is independent).**

### Phase 3: Function & Method Improvements — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 09 | [x] | [09-named-args-defaults.md](09-named-args-defaults.md) | Named args, defaults, shorthand | — |
| 10 | [x] | [10-methods.md](10-methods.md) | `impl` blocks, `self`, method calls | — |

> **09 and 10 can be done in parallel.**

### Phase 4: Enums & Pattern Matching — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 11 | [x] | [11-enums.md](11-enums.md) | Enum types (unit, data, struct variants) | — |
| 12 | [x] | [12-pattern-matching.md](12-pattern-matching.md) | `match`, `if let`, patterns, exhaustiveness | 06, 11 |

> **11 first, then 12. 12 also uses tuples (06) for tuple patterns.**

### Phase 5: Generics & Interfaces — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 13 | [x] | [13-generic-functions.md](13-generic-functions.md) | Generic functions, type inference, monomorphization | — |
| 14 | [x] | [14-generic-types.md](14-generic-types.md) | Generic structs/enums, turbofish `::< >` | 11, 13 |
| 15 | [x] | [15-interfaces.md](15-interfaces.md) | `interface` decl, `impl for`, bounds, where clauses | 13, 14 |
| 16 | [x] | [16-dyn-dispatch-operators.md](16-dyn-dispatch-operators.md) | `dyn Interface`, vtables, operator overloading | 11, 15 |

> **Sequential chain: 13 → 14 → 15 → 16. Task 14 also needs enums (11).**

### Phase 6: Collections — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 17 | [x] | [17-dynamic-arrays.md](17-dynamic-arrays.md) | `T[]` with push/pop/index | 07, 13-14 (generics for methods) |
| 18 | [x] | [18-map-type.md](18-map-type.md) | `Map<K, V>` with insert/get/remove | 13-14, 21 (generics, Option for `get`) |

> **17 and 18 can be done in parallel. Both use generic infrastructure from Phase 5. 17 also reuses index IR from 07.**

### Phase 7: Higher-Order Functions — [~] Partial
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 19 | [~] | [19-closures.md](19-closures.md) | Closures parsed/type-checked, placeholder lowering (no captures yet) | — |
| 20 | [ ] | [20-trailing-lambdas.md](20-trailing-lambdas.md) | Trailing lambda syntax, implicit `it` | 19 |

> **19 parser + checker done; lowering is placeholder (inline body, no capture struct). 20 not started.**

### Phase 8: Error Handling — [~] Partial
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 21 | [~] | [21-option-result-error-prop.md](21-option-result-error-prop.md) | Option/Result defined in stdlib prelude; `?` operator parsed/checked (passthrough); `panic`/`assert`/`exit` builtins | 11, 12, 14 |

> **Option<T>/Result<T,E> defined in nudl-std/prelude.nudl. `?` operator parsed and type-checked (simplified passthrough). panic/assert/exit registered as builtins.**

### Phase 9: Strings — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 22 | [x] | [22-string-runtime-and-templates.md](22-string-runtime-and-templates.md) | Template string lowering via __str_concat and __*_to_str builtins | — |

> **Template strings fully lowered: string parts interleaved with expression-to-string conversions chained via __str_concat.**

### Phase 10: Memory Management — [~] Partial
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 23 | [~] | [23-arc-and-defer.md](23-arc-and-defer.md) | `defer` statement implemented; ARC retain/release for structs/enums working | 11 (enum destructors) |

> **`defer { ... }` parsed, type-checked, lowered (deferred blocks emitted in LIFO order before function return). ARC was already working for struct/enum types.**

### Phase 11: Module System & Stdlib — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 24 | [x] | [24-destructuring.md](24-destructuring.md) | Let destructuring for tuples and structs via LetPattern | 06, 12 |
| 25 | [x] | [25-module-system.md](25-module-system.md) | Multi-file imports, path resolution, nudl-std search | — |
| 26 | [x] | [26-stdlib.md](26-stdlib.md) | std::prelude, std::math, std::string, std::io, std::collections | 22, 25, 04 |

> **All three tasks complete. Destructuring uses TupleLoad/Load IR instructions. Module system resolves imports relative to source dir and nudl-std/. Stdlib provides prelude (Option, Result, min, max, clamp), math, string, io, and collections modules.**

### Phase 12: VM — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 27 | [ ] | [27-vm-updates.md](27-vm-updates.md) | VM support for all new IR instructions | All above |

> **Can be done incrementally alongside each task, or as a final sweep.**

## Dependency Graph (Critical Path)

```
Phase 1: [01] [02] [03] [04] [05]          (all parallel)
Phase 2: [06] [07] → [08]                  (06∥07, then 08)
Phase 3: [09] [10]                          (parallel)
Phase 4: [11] → [12]                       (12 needs 06+11)
Phase 5: [13] → [14] → [15] → [16]        (sequential chain, 14 needs 11)
Phase 6: [17] [18]                          (parallel, both need generics; 17 also reuses 07; 18 needs 21 for Option)
Phase 7: [19] → [20]
Phase 8: [21]                               (needs 11+12+14 for generic Option/Result)
Phase 9: [22]                               (independent, parallel with 4-8)
Phase 10: [23]                              (needs 11)
Phase 11: [24] [25] → [26]                 (26 needs 22+25)
Phase 12: [27]                              (final sweep)
```

**Critical path:** 11 → 13 → 14 → 15 → 16, and 11 → 12 → 21

## Maximum Parallelism Schedule

| Wave | Tasks | Notes |
|------|-------|-------|
| Wave 1 | 01, 02, 03, 04, 05, 06, 07, 09, 10, 22 | All independent |
| Wave 2 | 08, 11, 19, 25 | 08 needs 07; others independent |
| Wave 3 | 12, 13, 20, 23, 24 | 12 needs 06+11; 13 independent; 20 needs 19; 23 needs 11; 24 needs 06+12 |
| Wave 4 | 14 | Needs 11+13 |
| Wave 5 | 15, 17, 21 | 15 needs 13+14; 17 needs 07+13+14; 21 needs 11+12+14 |
| Wave 6 | 16, 18, 26 | 16 needs 11+15; 18 needs 13+14+21; 26 needs 22+25 |
| Wave 7 | 27 | Final VM sweep |

## Verification Plan
1. `cargo test --workspace` — all existing tests pass after each task
2. Run each test file in `tests/` directories for implemented features
3. Integration test: program using template strings, stdlib imports, for-loops, match, closures, structs with methods, generics, interfaces, arrays, and C FFI
4. ARC verification: program creating/destroying many heap objects
5. Module system: multi-file project importing from std
6. Generics verification: monomorphization produces correct specialized code
7. Interface verification: static dispatch and dyn dispatch both work correctly

## Key Architecture Decisions
- **Methods**: Mangled names (`Type__method`), self as first param, no vtable for static dispatch
- **Generics**: Monomorphization (no type erasure) — each instantiation is a separate function/type
- **Interfaces**: Static dispatch by default, `dyn Interface` for dynamic dispatch via vtables
- **Operator overloading**: Via built-in interfaces (`Add`, `Eq`, `Ord`, etc.), desugared to method calls
- **Closures**: Fat pointer (fn_ptr + heap-allocated ARC'd capture struct)
- **For loops**: While-loop desugaring (no Iterator interface in MVP)
- **Pattern matching**: Lowered to if-else chains
- **Arrays/Maps**: Concrete TypeKind variants (no monomorphization needed)
- **Enums**: Tag + payload heap objects
- **Module system**: Merge into single Program, pub-only exports
