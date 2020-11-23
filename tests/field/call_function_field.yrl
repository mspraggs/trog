// bar
// 0
class Foo {}
fn bar() {
  print("bar");
}
var foo = Foo();
foo.bar = bar;
foo.bar();
