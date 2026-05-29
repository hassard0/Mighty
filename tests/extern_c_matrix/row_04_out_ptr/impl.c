/*
 * Row 4: out-param via pointer — `extern c fn foo(p: *mut i32)`.
 *
 * Same wrapper rationale as row 03: Mighty's `*I32` parses as a
 * borrow-shaped type but the borrowck still tightens around taking
 * the address of a local for FFI. The wrapper allocates the slot,
 * passes its address, then surfaces the result via stdout.
 */
#include <stdint.h>
#include <stdio.h>

static void row04_real(int32_t *out) { *out = 42; }

int32_t mty_row04(void) {
  int32_t slot = 0;
  row04_real(&slot);
  printf("row04:out=%d\n", slot);
  fflush(stdout);
  return slot;
}
