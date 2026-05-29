/*
 * Row 6: struct by-pointer — `extern c fn foo(p: *const Point)`.
 *
 * The wrapper allocates a Point on its own stack, takes its address,
 * and forwards a pointer to the real helper. This is the shape every
 * "opaque handle" C API uses (HWND, FILE*, the WGPU device handle, etc).
 */
#include <stdint.h>
#include <stdio.h>

typedef struct {
  int32_t x;
  int32_t y;
} Point;

static int32_t row06_real(const Point *p) {
  printf("row06:px=%d,py=%d\n", p->x, p->y);
  fflush(stdout);
  return p->x * p->y;
}

int32_t mty_row06(void) {
  Point p = {6, 7};
  return row06_real(&p);
}
