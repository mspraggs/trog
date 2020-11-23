// A.method(arg)
// 0
class A {
  fn method(self, arg) {
    print("A.method(" + arg + ")");
  }
}
class B < A {
  fn getClosure(self) {
    return super.method;
  }
  fn method(self, arg) {
    print("B.method(" + arg + ")");
  }
}
var closure = B().getClosure();
closure("arg");
