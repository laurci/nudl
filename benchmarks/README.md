# Benchmarks: nudl vs C vs Swift

Performance comparison between nudl, C, and Swift, measured with [hyperfine](https://github.com/sharkdp/hyperfine).

All three languages compile through LLVM:
- **C** — `cc -O3 -march=native`
- **Swift** — `swiftc -O -whole-module-optimization`
- **nudl** — `--release --native` (LLVM `default<O3>` pass pipeline + host CPU targeting)

## Results

Measured on Apple M4 Pro, macOS 15.3.

### Fibonacci (recursive, n=42) — pure compute

No heap allocation, no ARC. Just recursive function calls and integer arithmetic.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C | 641.8 ± 6.2 | 626.6 | 648.6 | 1.00 |
| nudl | 648.4 ± 16.8 | 633.5 | 684.3 | 1.01 ± 0.03 |
| Swift | 852.6 ± 13.9 | 833.4 | 874.9 | 1.33 ± 0.03 |

**nudl matches C and beats Swift by 33%.** All three use LLVM, but nudl and C produce more optimal IR for this workload. Swift's overhead likely comes from its runtime function call conventions.

### Struct Point (10M iterations) — ARC overhead

Create a `Point { x, y }` in a loop, accumulate via a method call. Tests stack allocation vs heap + ARC.

The Swift version is benchmarked twice: as a `struct` (value type, stack-allocated like C) and as a `final class` (reference type, heap + ARC like nudl).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C (stack) | 1.5 ± 0.2 | 1.2 | 2.2 | 1.00 |
| Swift struct (stack) | 5.9 ± 0.5 | 5.3 | 13.9 | 3.87 ± 0.55 |
| Swift class (ARC) | 257.9 ± 1.2 | 255.9 | 259.6 | 170.50 ± 18.39 |
| nudl (ARC) | 398.7 ± 1.8 | 395.7 | 401.6 | 263.62 ± 28.43 |

**Apples-to-apples (ARC vs ARC): nudl is 1.55x slower than Swift class.** Swift's ARC optimizer is more mature — it can elide redundant retain/release pairs and uses atomic operations tuned for Apple silicon. The gap between C and both ARC variants shows the fundamental cost of heap allocation at scale.

The Swift struct result (5.9ms vs C's 1.5ms) shows that even value types carry some overhead vs C's ability to strength-reduce the entire loop away.

### Array Sum (10M push + sum) — collection runtime

Push 10M integers into a dynamic array, then iterate and sum.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C | 12.6 ± 1.3 | 11.8 | 28.5 | 1.00 |
| nudl | 27.5 ± 2.3 | 26.0 | 43.1 | 2.19 ± 0.28 |
| Swift | 28.0 ± 1.1 | 25.8 | 34.0 | 2.23 ± 0.24 |

**nudl and Swift are tied**, both ~2.2x slower than C. The overhead comes from bounds checking, growth logic, and function call overhead per operation — costs shared by any safe dynamic array implementation.

## Summary

| Benchmark | nudl vs C | nudl vs Swift | Notes |
|:---|:---|:---|:---|
| Pure compute | **1.01x** | **0.76x (faster)** | nudl matches C, beats Swift |
| Struct churn (ARC) | 264x | 1.55x | Swift has a more mature ARC optimizer |
| Dynamic arrays | 2.2x | **1.0x (tied)** | Same ballpark as Swift's Array |

The takeaway: nudl's LLVM codegen is solid — matching or beating Swift for compute and collections. The remaining gap is in ARC optimization, where Swift has years of work on retain/release elision and atomic refcount tuning.

## Running

Requires [hyperfine](https://github.com/sharkdp/hyperfine) (`brew install hyperfine`).

```bash
cd benchmarks
make          # build all (C, Swift, nudl)
make bench    # run all benchmarks
make bench-md # run and export markdown tables to results/
make clean    # remove binaries and results
```

## Benchmark descriptions

| File | What it measures |
|:---|:---|
| `fib.{c,swift,nudl}` | Recursive fibonacci(42). Pure stack computation, no heap. |
| `struct_point.{c,nudl}` | 10M struct allocations with method calls. C uses stack, nudl uses ARC. |
| `struct_point.swift` | Swift value type (struct) — stack-allocated like C. |
| `struct_point_class.swift` | Swift reference type (final class) — heap + ARC like nudl. |
| `array_sum.{c,swift,nudl}` | 10M dynamic array pushes + iteration. Measures collection runtime. |
