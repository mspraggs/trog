// A.method(arg)
// 0
class A {
  fn method(self, arg) {
    print("A.method(" + arg + ")");
  }
}
class B < A {
  fn get_closure(self) {
    return super.method;
  }
  fn method(self, arg) {
    print("B.method(" + arg + ")");
  }
}
var closure = B().get_closure();
closure("arg");
