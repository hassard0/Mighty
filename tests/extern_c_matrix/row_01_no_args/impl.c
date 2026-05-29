/*
 * Row 1: extern c fn mty_row01() -> i32 (no args).
 *
 * Prints "row01:42\n" to stdout when called. The Mighty side calls
 * this from main and exits; the test harness captures stdout and
 * asserts on the line.
 *
 * Why print rather than return-and-exit-code? Mighty's `main` lowers
 * to a C `main()` that already returns 0; we can't easily route an
 * arbitrary i32 back through the process exit. Printing a marker line
 * is the most portable, harness-friendly signal.
 */
#include <stdint.h>
#include <stdio.h>

int32_t mty_row01(void) {
  fputs("row01:42\n", stdout);
  fflush(stdout);
  return 42;
}
