// Expected 2 arguments but got 4.
// [line 10] in script
// 70
class Foo {
  fn __init__(self, a, b) {
    self.a = a;
    self.b = b;
  }
}
var foo = Foo(1, 2, 3, 4);
