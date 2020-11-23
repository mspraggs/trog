// foo
// bar
// bar
// 0
class Foo {
  fn methodOnFoo(self) { print("foo"); }
  fn override(self) { print("foo"); }
}
class Bar < Foo {
  fn methodOnBar(self) { print("bar"); }
  fn override(self) { print("bar"); }
}
var bar = Bar();
bar.methodOnFoo();
bar.methodOnBar();
bar.override();
