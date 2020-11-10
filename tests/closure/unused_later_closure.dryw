// a
// 0
var closure;
{
  var a = "a";
  {
    var b = "b";
    fn returnA() {
      return a;
    }
    closure = returnA;
    if (false) {
      fn returnB() {
        return b;
      }
    }
  }
  print(closure());
}
