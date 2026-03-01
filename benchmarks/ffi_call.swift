// FFI call benchmark — Swift calling C function

let n: Int64 = 50_000_000
var sum: Int64 = 0
for i: Int64 in 0..<n {
    sum &+= ffi_compute(i)
}
print(sum)
