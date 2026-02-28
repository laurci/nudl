CC      = cc
CFLAGS  = -O3 -march=native
SWIFTC  = swiftc
SFLAGS  = -O -whole-module-optimization
NUDL    = cargo run --bin nudl --

BENCHMARKS = fib struct_point array_sum

C_BINS     = $(addprefix bin/,$(addsuffix _c,$(BENCHMARKS)))
SWIFT_BINS = $(addprefix bin/,$(addsuffix _swift,$(BENCHMARKS)))
NUDL_BINS  = $(addprefix bin/,$(addsuffix _nudl,$(BENCHMARKS)))

.PHONY: all c swift nudl bench bench-md clean

all: c swift nudl

c: $(C_BINS) bin/ffi_call_c
swift: $(SWIFT_BINS) bin/struct_point_class_swift bin/ffi_call_swift
nudl: $(NUDL_BINS) bin/ffi_call_nudl

bin:
	mkdir -p bin

# --- Shared FFI library (separate object, no inlining) ---

bin/ffi_lib.o: ffi_lib.c ffi_lib.h | bin
	$(CC) $(CFLAGS) -c -o $@ $<

# --- C targets ---

bin/%_c: %.c | bin
	$(CC) $(CFLAGS) -o $@ $<

bin/ffi_call_c: ffi_call.c bin/ffi_lib.o | bin
	$(CC) $(CFLAGS) -o $@ $< bin/ffi_lib.o

# --- Swift targets ---

bin/%_swift: %.swift | bin
	$(SWIFTC) $(SFLAGS) -o $@ $<

bin/ffi_call_swift: ffi_call.swift bin/ffi_lib.o | bin
	$(SWIFTC) $(SFLAGS) -import-objc-header ffi_lib.h -o $@ $< bin/ffi_lib.o

# --- nudl targets ---

bin/%_nudl: %.nudl | bin
	cd .. && $(NUDL) build --release --native -o benchmarks/$@ benchmarks/$<

bin/ffi_call_nudl: ffi_call.nudl bin/ffi_lib.o | bin
	cd .. && $(NUDL) build --release --native --link benchmarks/bin/ffi_lib.o -o benchmarks/$@ benchmarks/$<

# --- Benchmarks ---

bench: all
	@echo "=== Fibonacci (recursive, n=42) — pure compute ==="
	hyperfine --warmup 3 -n 'C' './bin/fib_c > /dev/null' -n 'Swift' './bin/fib_swift > /dev/null' -n 'nudl' './bin/fib_nudl > /dev/null'
	@echo ""
	@echo "=== Struct Point (10M iterations) — ARC overhead ==="
	hyperfine --warmup 3 -n 'C (stack)' './bin/struct_point_c > /dev/null' -n 'Swift struct (stack)' './bin/struct_point_swift > /dev/null' -n 'Swift class (ARC)' './bin/struct_point_class_swift > /dev/null' -n 'nudl (ARC)' './bin/struct_point_nudl > /dev/null'
	@echo ""
	@echo "=== Array Sum (10M push + sum) — collection runtime ==="
	hyperfine --warmup 3 -n 'C' './bin/array_sum_c > /dev/null' -n 'Swift' './bin/array_sum_swift > /dev/null' -n 'nudl' './bin/array_sum_nudl > /dev/null'
	@echo ""
	@echo "=== FFI Call (50M calls to C function) — foreign function overhead ==="
	hyperfine --warmup 3 -n 'C' './bin/ffi_call_c > /dev/null' -n 'Swift' './bin/ffi_call_swift > /dev/null' -n 'nudl' './bin/ffi_call_nudl > /dev/null'

bench-md: all
	@mkdir -p results
	hyperfine --warmup 10 -n 'C' './bin/fib_c > /dev/null' -n 'Swift' './bin/fib_swift > /dev/null' -n 'nudl' './bin/fib_nudl > /dev/null' --export-markdown results/fib.md
	hyperfine --warmup 10 -n 'C (stack)' './bin/struct_point_c > /dev/null' -n 'Swift struct (stack)' './bin/struct_point_swift > /dev/null' -n 'Swift class (ARC)' './bin/struct_point_class_swift > /dev/null' -n 'nudl (ARC)' './bin/struct_point_nudl > /dev/null' --export-markdown results/struct_point.md
	hyperfine --warmup 10 -n 'C' './bin/array_sum_c > /dev/null' -n 'Swift' './bin/array_sum_swift > /dev/null' -n 'nudl' './bin/array_sum_nudl > /dev/null' --export-markdown results/array_sum.md
	hyperfine --warmup 10 -n 'C' './bin/ffi_call_c > /dev/null' -n 'Swift' './bin/ffi_call_swift > /dev/null' -n 'nudl' './bin/ffi_call_nudl > /dev/null' --export-markdown results/ffi_call.md

clean:
	rm -rf bin results
