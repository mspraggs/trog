// Undefined property 'does_not_exist'.
// [line 8] in foo()
// [line 11] in script
// 70
class Base {}
class Derived < Base {
  fn foo(self) {
    super.does_not_exist(1);
  }
}
Derived().foo();
