/* C harness for native_abi/01_export_main.
 *
 * Compile + link:
 *   mty build --target native --emit obj -o input.o input.mty
 *   cc harness.c input.o -o harness
 *   ./harness          ; echo $?
 *
 * Conformant impls produce a harness that exits 42 (the sum 40 + 2
 * computed inside the Mighty fn).
 */
#include <stdint.h>
#include <stdio.h>

extern int32_t _add(int32_t a, int32_t b);

int main(void) {
  int32_t r = _add(40, 2);
  return (int)r;
}
