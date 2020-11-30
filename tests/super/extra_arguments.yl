// Derived.foo()
// Expected 2 arguments but got 4.
// [line 14] in foo()
// [line 17] in script
// 70
class Base {
  fn foo(self, a, b) {
    print("Base.foo(" + a + ", " + b + ")");
  }
}
class Derived < Base {
  fn foo(self) {
    print("Derived.foo()");
    super.foo("a", "b", "c", "d");
  }
}
Derived().foo();
