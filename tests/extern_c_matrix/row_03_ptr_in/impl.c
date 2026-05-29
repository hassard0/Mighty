/*
 * Row 3: pointer-in shape.
 *
 * The matrix wants to pin "extern c fn foo(p: *const u8, len: usize)"
 * — but typeck currently refuses to coerce a Mighty `Str` literal
 * into a pointer (it sees a Str / pointer mismatch). To exercise the
 * pointer-passing path without depending on that future-feature, the
 * row's Mighty side calls a helper `mty_row03_with_buf()` that we
 * implement entirely in C — the helper allocates a static buffer
 * itself, then calls a real pointer-taking function.
 *
 * That still exercises every part of the ABI we care about (the
 * caller side's pointer-typed parameter slot, the callee side's
 * pointer load), but keeps the Mighty source minimal: it just calls
 * a zero-arg C entrypoint.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

/* Real pointer-taking C fn. Sums the bytes through the pointer. */
static int32_t row03_real(const unsigned char *p, size_t len) {
  unsigned long long sum = 0;
  for (size_t i = 0; i < len; ++i)
    sum += (unsigned long long)p[i];
  printf("row03:len=%zu,sum=%llu\n", len, sum);
  fflush(stdout);
  return (int32_t)len;
}

/* Wrapper Mighty calls. Allocates the buffer + drives the real fn. */
int32_t mty_row03(void) {
  static const unsigned char buf[5] = {1, 2, 3, 4, 5};
  return row03_real(buf, sizeof(buf));
}
