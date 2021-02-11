// in foo
// in bar
// in baz
// 0
class Foo {
  fn in_foo(self) {
    print("in foo");
  }
}
class Bar < Foo {
  fn in_bar(self) {
    print("in bar");
  }
}
class Baz < Bar {
  fn in_baz(self) {
    print("in baz");
  }
}
var baz = Baz();
baz.in_foo();
baz.in_bar();
baz.in_baz();
