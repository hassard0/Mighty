/*
 * Row 2: extern c fn mty_row02(a: i32, b: i32) -> i32.
 *
 * Pins both register-passed I32 args (System V on Unix maps these to
 * %edi/%esi; Windows fastcall to %ecx/%edx). Prints both inputs and
 * the sum so the harness can detect any argument-swap regression.
 */
#include <stdint.h>
#include <stdio.h>

int32_t mty_row02(int32_t a, int32_t b) {
  printf("row02:%d+%d=%d\n", a, b, a + b);
  fflush(stdout);
  return a + b;
}
