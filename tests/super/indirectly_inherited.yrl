// C.foo()
// A.foo()
// 0
class A {
  fn foo(self) {
    print("A.foo()");
  }
}
class B < A {}
class C < B {
  fn foo(self) {
    print("C.foo()");
    super.foo();
  }
}
C().foo();
