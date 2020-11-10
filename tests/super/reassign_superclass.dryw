// Base.method()
// Base.method()
// 0
class Base {
  fn method(self) {
    print("Base.method()");
  }
}
class Derived < Base {
  fn method(self) {
    super.method();
  }
}
class OtherBase {
  fn method(self) {
    print("OtherBase.method()");
  }
}
var derived = Derived();
derived.method();
Base = OtherBase;
derived.method();
