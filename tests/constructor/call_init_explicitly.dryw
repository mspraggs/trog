// Foo.__init__(one)
// Foo.__init__(two)
// Foo instance
// __init__
// 0
class Foo {
  fn __init__(self, arg) {
    print("Foo.__init__(" + arg + ")");
    self.field = "__init__";
  }
}
var foo = Foo("one");
foo.field = "field";
var foo2 = foo.__init__("two");
print(foo2);
print(foo.field);
