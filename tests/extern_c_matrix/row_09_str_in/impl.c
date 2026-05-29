/*
 * Row 9: Mighty Str → C `const char *`.
 *
 * Mighty's `Const::Str` interns the literal as a null-terminated UTF-8
 * blob and yields its address as a pointer. The cranelift backend
 * passes that pointer straight through any extern call expecting a
 * 64-bit value — which `const char *` is on every host we target.
 *
 * Wrapper-pattern: the Mighty side calls a zero-arg helper, the C
 * side allocates the string and forwards a pointer to the real fn.
 * This pins the `Str ↔ const char*` shape end-to-end even though
 * typeck won't yet coerce a `Str` literal directly into a `*U8` param.
 */
#include <stddef.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>

static int32_t row09_real(const char *s) {
  printf("row09:s=%s,len=%zu\n", s, strlen(s));
  fflush(stdout);
  return (int32_t)strlen(s);
}

int32_t mty_row09(void) {
  return row09_real("hello-from-c");
}
