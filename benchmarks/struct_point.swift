// Struct point accumulation — Swift value type (stack-allocated, like C)

struct Point {
    var x: Int64
    var y: Int64

    func add(_ other: Point) -> Point {
        return Point(x: self.x + other.x, y: self.y + other.y)
    }
}

let n: Int64 = 10_000_000
var p = Point(x: 0, y: 0)
for i: Int64 in 0..<n {
    p = p.add(Point(x: i, y: i * 2))
}
print("\(p.x), \(p.y)")
