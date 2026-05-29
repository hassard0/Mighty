/*
 * Row 10: caller-owned out-buffer — `extern c fn foo(s: *mut Str) -> usize`.
 *
 * The caller (Mighty side via the wrapper) provides a sized buffer;
 * the callee writes UTF-8 bytes into it and returns the bytes-written
 * count. This is the C convention every "write me a string here"
 * API uses (`strncpy`, `snprintf`, etc).
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static size_t row10_real(char *buf, size_t cap) {
  const char msg[] = "row10";
  size_t n = sizeof(msg) - 1;
  if (n > cap)
    n = cap;
  memcpy(buf, msg, n);
  if (n < cap)
    buf[n] = 0;
  return n;
}

int32_t mty_row10(void) {
  char buf[16] = {0};
  size_t n = row10_real(buf, sizeof(buf));
  printf("row10:wrote=%zu,buf=%s\n", n, buf);
  fflush(stdout);
  return (int32_t)n;
}
