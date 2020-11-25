// a
// 0
var closure;
{
  var a = "a";
  {
    var b = "b";
    fn return_a() {
      return a;
    }
    closure = return_a;
    if (false) {
      fn return_b() {
        return b;
      }
    }
  }
  print(closure());
}
