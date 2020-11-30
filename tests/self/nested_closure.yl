// Foo
// 0
class Foo {
  fn get_closure(self) {
    fn f() {
      fn g() {
        fn h() {
          return self.to_string();
        }
        return h;
      }
      return g;
    }
    return f;
  }
  fn to_string(self) { return "Foo"; }
}
var closure = Foo().get_closure();
print(closure()()());
