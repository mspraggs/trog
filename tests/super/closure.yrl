// Base
// 0
class Base {
  fn toString(self) { return "Base"; }
}
class Derived < Base {
  fn getClosure(self) {
    fn closure() {
      return super.toString();
    }
    return closure;
  }
  fn toString(self) { return "Derived"; }
}
var closure = Derived().getClosure();
print(closure());
