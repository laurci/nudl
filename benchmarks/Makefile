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

c: $(C_BINS)
swift: $(SWIFT_BINS) bin/struct_point_class_swift
nudl: $(NUDL_BINS)

bin:
	mkdir -p bin

# --- C targets ---

bin/%_c: %.c | bin
	$(CC) $(CFLAGS) -o $@ $<

# --- Swift targets ---

bin/%_swift: %.swift | bin
	$(SWIFTC) $(SFLAGS) -o $@ $<

# --- nudl targets ---

bin/%_nudl: %.nudl | bin
	cd .. && $(NUDL) build --release --native -o benchmarks/$@ benchmarks/$<

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

bench-md: all
	@mkdir -p results
	hyperfine --warmup 10 -n 'C' './bin/fib_c > /dev/null' -n 'Swift' './bin/fib_swift > /dev/null' -n 'nudl' './bin/fib_nudl > /dev/null' --export-markdown results/fib.md
	hyperfine --warmup 10 -n 'C (stack)' './bin/struct_point_c > /dev/null' -n 'Swift struct (stack)' './bin/struct_point_swift > /dev/null' -n 'Swift class (ARC)' './bin/struct_point_class_swift > /dev/null' -n 'nudl (ARC)' './bin/struct_point_nudl > /dev/null' --export-markdown results/struct_point.md
	hyperfine --warmup 10 -n 'C' './bin/array_sum_c > /dev/null' -n 'Swift' './bin/array_sum_swift > /dev/null' -n 'nudl' './bin/array_sum_nudl > /dev/null' --export-markdown results/array_sum.md

clean:
	rm -rf bin results
