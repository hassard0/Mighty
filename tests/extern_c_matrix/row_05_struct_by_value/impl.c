/*
 * Row 5: struct by-value — `extern c fn foo(p: Point) -> i32`.
 *
 * The struct is small enough (8 bytes) to ride the SystemV / WinFastcall
 * register-passing path. The wrapper constructs the value and forwards
 * it so the Mighty source stays minimal (we don't yet expose struct
 * literal construction over FFI in a way the borrow checker accepts).
 */
#include <stdint.h>
#include <stdio.h>

typedef struct {
  int32_t x;
  int32_t y;
} Point;

static int32_t row05_real(Point p) {
  printf("row05:x=%d,y=%d\n", p.x, p.y);
  fflush(stdout);
  return p.x + p.y;
}

int32_t mty_row05(void) {
  Point p = {3, 4};
  return row05_real(p);
}
