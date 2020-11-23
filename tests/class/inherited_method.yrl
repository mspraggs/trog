// in foo
// in bar
// in baz
// 0
class Foo {
  fn inFoo(self) {
    print("in foo");
  }
}
class Bar < Foo {
  fn inBar(self) {
    print("in bar");
  }
}
class Baz < Bar {
  fn inBaz(self) {
    print("in baz");
  }
}
var baz = Baz();
baz.inFoo();
baz.inBar();
baz.inBaz();
