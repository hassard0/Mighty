/*
 * Row 8: fixed-size array pointer — `extern c fn foo(arr: *const [i32; 4])`.
 *
 * Wrapper allocates a 4-int array on the stack, takes its address as
 * `int(*)[4]`, and forwards to the real fn. Pins that the cranelift
 * backend passes the array-pointer parameter the same way as any
 * other pointer (i64 on all targets) — `[i32; 4]*` and `int*` use
 * identical SystemV / Win64 slots.
 */
#include <stdint.h>
#include <stdio.h>

static int32_t row08_real(const int32_t (*arr)[4]) {
  int32_t sum = 0;
  for (int i = 0; i < 4; ++i)
    sum += (*arr)[i];
  printf("row08:sum=%d\n", sum);
  fflush(stdout);
  return sum;
}

int32_t mty_row08(void) {
  int32_t arr[4] = {1, 2, 3, 4};
  return row08_real(&arr);
}
