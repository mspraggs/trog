// Foo
// 0
class Foo {
  fn getClosure(self) {
    fn f() {
      fn g() {
        fn h() {
          return self.toString();
        }
        return h;
      }
      return g;
    }
    return f;
  }
  fn toString(self) { return "Foo"; }
}
var closure = Foo().getClosure();
print(closure()()());
