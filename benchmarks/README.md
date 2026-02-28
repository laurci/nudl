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
| C | 646.5 ± 8.6 | 636.0 | 660.8 | 1.00 |
| nudl | 652.9 ± 9.2 | 638.4 | 667.8 | 1.01 ± 0.02 |
| Swift | 870.2 ± 26.0 | 826.6 | 911.8 | 1.35 ± 0.04 |

**nudl matches C and beats Swift by 35%.** All three use LLVM, but nudl and C produce more optimal IR for this workload. Swift's overhead likely comes from its runtime function call conventions.

### Struct Point (10M iterations) — ARC overhead

Create a `Point { x, y }` in a loop, accumulate via a method call. Tests stack allocation vs heap + ARC.

The Swift version is benchmarked twice: as a `struct` (value type, stack-allocated like C) and as a `final class` (reference type, heap + ARC like nudl).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C (stack) | 1.5 ± 0.3 | 1.1 | 7.5 | 1.00 |
| Swift struct (stack) | 5.8 ± 0.6 | 5.3 | 14.0 | 3.82 ± 0.89 |
| Swift class (ARC) | 269.6 ± 12.4 | 258.9 | 295.7 | 176.74 ± 37.27 |
| nudl (ARC) | 413.8 ± 15.4 | 400.3 | 446.7 | 271.29 ± 56.72 |

**Apples-to-apples (ARC vs ARC): nudl is 1.53x slower than Swift class.** Swift's ARC optimizer is more mature — it can elide redundant retain/release pairs and uses atomic operations tuned for Apple silicon. The gap between C and both ARC variants shows the fundamental cost of heap allocation at scale.

The Swift struct result (5.8ms vs C's 1.5ms) shows that even value types carry some overhead vs C's ability to strength-reduce the entire loop away.

### Array Sum (10M push + sum) — collection runtime

Push 10M integers into a dynamic array, then iterate and sum.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C | 12.3 ± 1.4 | 10.8 | 26.2 | 1.00 |
| Swift | 26.2 ± 2.5 | 23.4 | 40.9 | 2.12 ± 0.32 |
| nudl | 26.5 ± 2.6 | 24.4 | 41.7 | 2.15 ± 0.33 |

**nudl and Swift are tied**, both ~2.1x slower than C. The overhead comes from bounds checking, growth logic, and function call overhead per operation — costs shared by any safe dynamic array implementation.

### FFI Call (50M calls to C function) — foreign function overhead

Call `ffi_compute(x) → x*x + x` defined in a separately compiled C object file. Measures the overhead of crossing the FFI boundary.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| C | 41.1 ± 2.5 | 39.2 | 52.9 | 1.00 |
| nudl | 41.2 ± 2.6 | 39.4 | 56.1 | 1.00 ± 0.09 |
| Swift | 42.1 ± 3.2 | 40.1 | 63.8 | 1.02 ± 0.10 |

**All three are identical.** There is zero FFI overhead — calling a C function from nudl or Swift is exactly as fast as calling it from C. This is because all three compile to native code and use the same C calling convention. The "FFI boundary" is just a normal function call at the machine level.

## Summary

| Benchmark | nudl vs C | nudl vs Swift | Notes |
|:---|:---|:---|:---|
| Pure compute | **1.01x** | **0.75x (faster)** | nudl matches C, beats Swift |
| Struct churn (ARC) | 271x | 1.53x | Swift has a more mature ARC optimizer |
| Dynamic arrays | 2.15x | **1.0x (tied)** | Same ballpark as Swift's Array |
| FFI calls | **1.00x** | **1.0x (tied)** | Zero FFI overhead in all three |

The takeaway: nudl's LLVM codegen is solid — matching or beating Swift for compute, collections, and FFI. The remaining gap is in ARC optimization, where Swift has years of work on retain/release elision and atomic refcount tuning.

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
| `ffi_call.{c,swift,nudl}` | 50M calls to a C function in a separate object. Measures FFI overhead. |
| `ffi_lib.{c,h}` | Shared C library used by all FFI benchmarks. |
