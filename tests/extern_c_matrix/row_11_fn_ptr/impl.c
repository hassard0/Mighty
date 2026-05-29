/*
 * Row 11: function pointer — `extern c fn foo(cb: extern fn(i32) -> i32)`.
 *
 * The Mighty side calls a wrapper; the C wrapper synthesises a
 * callback locally and forwards both the cb pointer and an argument
 * to the real fn. The "real" fn invokes the callback.
 */
#include <stdint.h>
#include <stdio.h>

typedef int32_t (*Cb)(int32_t);

static int32_t local_cb(int32_t x) { return x * 2 + 1; }

static int32_t row11_real(Cb cb, int32_t x) {
  int32_t r = cb(x);
  printf("row11:cb(%d)=%d\n", x, r);
  fflush(stdout);
  return r;
}

int32_t mty_row11(void) { return row11_real(local_cb, 20); }
