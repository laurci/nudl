// Dynamic array push + sum — measures collection/runtime overhead

let n: Int64 = 10_000_000
var arr: [Int64] = []

for i: Int64 in 0..<n {
    arr.append(i)
}

var sum: Int64 = 0
for x in arr {
    sum += x
}

print(sum)
