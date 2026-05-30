/* repro_print.c — tiny FFI int printer for the Vec.push repro.
 *
 * Mighty native `log` accepts only string literals (L23), so a computed
 * `v.len()` must be printed through an FFI scalar printer. This is the only
 * external symbol the repro binary calls.
 */
#include <stdio.h>
#include <stdint.h>

void repro_print_i32(int32_t v) {
  printf("repro: v.len()=%d\n", (int)v);
  fflush(stdout);
}
