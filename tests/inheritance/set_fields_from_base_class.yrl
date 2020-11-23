// foo 1
// foo 2
// bar 1
// bar 2
// bar 1
// bar 2
// 0
class Foo {
fn foo(self, a, b) {
    self.field1 = a;
    self.field2 = b;
  }
fn fooPrint(self) {
    print(self.field1);
    print(self.field2);
  }
}
class Bar < Foo {
  fn bar(self, a, b) {
    self.field1 = a;
    self.field2 = b;
  }
  fn barPrint(self) {
    print(self.field1);
    print(self.field2);
  }
}
var bar = Bar();
bar.foo("foo 1", "foo 2");
bar.fooPrint();
bar.bar("bar 1", "bar 2");
bar.barPrint();
bar.fooPrint();
