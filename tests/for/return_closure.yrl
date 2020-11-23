// i
// 0
fn f() {
  for _ in 1..3 {
    var i = "i";
    fn g() { print(i); }
    return g;
  }
}
var h = f();
h();
