// Recursive Fibonacci — pure compute benchmark (no heap allocation)

func fib(_ n: Int64) -> Int64 {
    if n <= 1 { return n }
    return fib(n - 1) + fib(n - 2)
}

let result = fib(42)
print(result)
