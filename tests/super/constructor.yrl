// Derived.__init__()
// Base.__init__(a, b)
// 0
class Base {
  fn __init__(self, a, b) {
    print("Base.__init__(" + a + ", " + b + ")");
  }
}
class Derived < Base {
  fn __init__(self) {
    print("Derived.__init__()");
    super.__init__("a", "b");
  }
}
Derived();
