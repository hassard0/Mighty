/*
 * Row 7: return struct by-value — `extern c fn foo() -> Point`.
 *
 * Small (8-byte) structs ride a single integer register on every host
 * ABI we target; larger structs come back via a hidden first pointer
 * (the "sret" convention). This row exercises the small-struct path.
 *
 * Wrapper forwards the result to a print statement so the harness can
 * confirm the bytes survived the return register.
 */
#include <stdint.h>
#include <stdio.h>

typedef struct {
  int32_t x;
  int32_t y;
} Point;

static Point row07_real(void) {
  Point p = {10, 32};
  return p;
}

int32_t mty_row07(void) {
  Point p = row07_real();
  printf("row07:rx=%d,ry=%d\n", p.x, p.y);
  fflush(stdout);
  return p.x + p.y;
}
