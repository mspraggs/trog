// Derived.foo()
// Base.foo()
// 0
class Base {
  fn foo(self) {
    print("Base.foo()");
  }
}
class Derived < Base {
  fn foo(self) {
    print("Derived.foo()");
    super.foo();
  }
}
Derived().foo();
