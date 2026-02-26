# nudl MVP Task Plan

## Goal
Complete all of STATUS.md §1-5 plus supporting infrastructure (enums, pattern matching, etc.), a basic module system, and a stdlib written in nudl using extern C. End result: a language supporting string interpolation, stdlib calls, ARC memory management, C FFI, closures, pattern matching, for-loops, methods, arrays, maps, enums, and more.

## Status Legend
- [ ] Not started
- [~] In progress
- [x] Complete

## Execution Order & Dependencies

### Phase 1: Foundation (independent, no cross-deps) — [x] Complete
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 01 | [x] | [01-type-cast-and-f32.md](01-type-cast-and-f32.md) | `as` operator + f32 IR/codegen | — |
| 02 | [x] | [02-bitwise-operators.md](02-bitwise-operators.md) | `&`, `\|`, `^`, `~` + compound assignments | — |
| 03 | [x] | [03-constants-labels-never.md](03-constants-labels-never.md) | `const`, labeled loops, `!` type | — |
| 04 | [x] | [04-ffi-types.md](04-ffi-types.md) | `MutRawPtr`, `CStr` types | — |
| 05 | [x] | [05-pipe-operator.md](05-pipe-operator.md) | `\|>` pipe operator | — |

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

### Phase 4: Enums & Pattern Matching — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 11 | [ ] | [11-enums.md](11-enums.md) | Enum types (unit, data, struct variants) | — |
| 12 | [ ] | [12-pattern-matching.md](12-pattern-matching.md) | `match`, `if let`, patterns, exhaustiveness | 06, 11 |

> **11 first, then 12. 12 also uses tuples (06) for tuple patterns.**

### Phase 5: Collections — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 13 | [ ] | [13-dynamic-arrays.md](13-dynamic-arrays.md) | `T[]` with push/pop/index | 07 (IndexLoad/IndexStore IR) |
| 14 | [ ] | [14-map-type.md](14-map-type.md) | `Map<K, V>` with insert/get/remove | — |

> **13 and 14 can be done in parallel (13 reuses index IR from 07).**

### Phase 6: Higher-Order Functions — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 15 | [ ] | [15-closures.md](15-closures.md) | Closures, captures, function values | — |
| 16 | [ ] | [16-trailing-lambdas.md](16-trailing-lambdas.md) | Trailing lambda syntax, implicit `it` | 15 |

> **15 first, then 16.**

### Phase 7: Error Handling — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 17 | [ ] | [17-option-result-error-prop.md](17-option-result-error-prop.md) | `Option<T>`, `Result<T,E>`, `?` operator | 11, 12 |

### Phase 8: Strings — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 18 | [ ] | [18-string-runtime-and-templates.md](18-string-runtime-and-templates.md) | String C runtime, template string lowering | — |

> **Can be done in parallel with Phases 4-7.**

### Phase 9: Memory Management — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 19 | [ ] | [19-arc-and-defer.md](19-arc-and-defer.md) | ARC dealloc, sharing, `defer` | 11 (enum destructors) |

### Phase 10: Module System & Stdlib — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 20 | [ ] | [20-destructuring.md](20-destructuring.md) | Let destructuring (tuple, struct) | 06, 12 |
| 21 | [ ] | [21-module-system.md](21-module-system.md) | Multi-file, imports, `pub`, stdlib paths | — |
| 22 | [ ] | [22-stdlib.md](22-stdlib.md) | std::io, std::math, std::string, prelude | 18, 21, 04 |

> **20 and 21 can be done in parallel. 22 depends on 21 (modules) and 18 (strings).**

### Phase 11: VM — [ ] Not started
| Task | Status | File | Summary | Depends On |
|------|--------|------|---------|------------|
| 23 | [ ] | [23-vm-updates.md](23-vm-updates.md) | VM support for all new IR instructions | All above |

> **Can be done incrementally alongside each task, or as a final sweep.**

## Dependency Graph (Critical Path)

```
Phase 1: [01] [02] [03] [04] [05]     (all parallel)
Phase 2: [06] [07] → [08]              (06∥07, then 08)
Phase 3: [09] [10]                      (parallel)
Phase 4: [11] → [12]                   (12 needs 06+11)
Phase 5: [13] [14]                      (parallel, 13 reuses 07)
Phase 6: [15] → [16]
Phase 7: [17]                           (needs 11+12)
Phase 8: [18]                           (independent, parallel with 4-7)
Phase 9: [19]                           (needs 11)
Phase 10: [20] [21] → [22]             (22 needs 18+21)
Phase 11: [23]                          (final sweep)
```

**Critical path:** 07 → 08, 11 → 12 → 17, 15 → 16, 21 → 22

## Maximum Parallelism Schedule

| Wave | Tasks | Notes |
|------|-------|-------|
| Wave 1 | 01, 02, 03, 04, 05, 06, 07, 09, 10, 18 | All independent |
| Wave 2 | 08, 11, 13, 14, 15, 21 | 08 needs 07; others independent |
| Wave 3 | 12, 16, 19, 20 | 12 needs 06+11; 16 needs 15; 19 needs 11; 20 needs 06+12 |
| Wave 4 | 17, 22 | 17 needs 11+12; 22 needs 18+21 |
| Wave 5 | 23 | Final VM sweep |

## Verification Plan
1. `cargo test --workspace` — all existing tests pass after each task
2. Run each test file in `tests/` directories for implemented features
3. Integration test: program using template strings, stdlib imports, for-loops, match, closures, structs with methods, arrays, and C FFI
4. ARC verification: program creating/destroying many heap objects
5. Module system: multi-file project importing from std

## Key Architecture Decisions
- **Methods**: Mangled names (`Type__method`), self as first param, no vtable
- **Closures**: Fat pointer (fn_ptr + heap-allocated ARC'd capture struct)
- **For loops**: While-loop desugaring (no Iterator interface in MVP)
- **Pattern matching**: Lowered to if-else chains
- **Arrays/Maps**: Concrete TypeKind variants (no monomorphization needed)
- **Enums**: Tag + payload heap objects
- **Module system**: Merge into single Program, pub-only exports
