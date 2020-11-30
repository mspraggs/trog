// Base
// 0
class Base {
  fn to_string(self) { return "Base"; }
}
class Derived < Base {
  fn get_closure(self) {
    fn closure() {
      return super.to_string();
    }
    return closure;
  }
  fn to_string(self) { return "Derived"; }
}
var closure = Derived().get_closure();
print(closure());
