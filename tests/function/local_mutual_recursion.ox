// Undefined variable 'isOdd'.
// [line 8] in isEven()
// [line 13] in script
// 70
{
  fun isEven(n) {
    if (n == 0) return true;
    return isOdd(n - 1);
  }
  fun isOdd(n) {
    return isEven(n - 1);
  }
  isEven(4);
}
