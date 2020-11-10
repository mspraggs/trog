// Derived.bar()
// Base.foo()
// 0
class Base {
  fn foo(self) {
    print("Base.foo()");
  }
}
class Derived < Base {
  fn bar(self) {
    print("Derived.bar()");
    super.foo();
  }
}
Derived().bar();
