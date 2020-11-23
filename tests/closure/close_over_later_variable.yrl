// b
// a
// 0
fn f() {
  var a = "a";
  var b = "b";
  fn g() {
    print(b);
    print(a);
  }
  g();
}
f();
