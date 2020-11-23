// Foo
// 0
{
  class Foo {
    fn returnSelf(self) {
      return Foo;
    }
  }
  print(Foo().returnSelf());
}
