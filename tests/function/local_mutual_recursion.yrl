// Undefined variable 'isOdd'.
// [line 8] in isEven()
// [line 13] in script
// 70
{
  fn isEven(n) {
    if n == 0 { return true; }
    return isOdd(n - 1);
  }
  fn isOdd(n) {
    return isEven(n - 1);
  }
  isEven(4);
}
