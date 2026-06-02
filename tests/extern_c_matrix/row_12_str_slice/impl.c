/*
 * Row 12: Mighty Str / String -> C (const char* ptr, size_t len).
 *
 * v0.46 T3 (L52 fix) — the cranelift backend now expands a Mighty Str
 * or String at an extern-c param slot into two ABI args (the ptr and
 * the byte length). The C declaration sees them as a normal pair:
 *
 *   void f(int64_t handle, const char* path_ptr, size_t path_len)
 *
 * The bytes are owned by the Mighty caller. C must not store the
 * pointer past the call. See `docs/internals/extern-c-matrix.md`
 * "Str slice (ptr, len) FFI" section.
 */
#include <stddef.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* Echo: copy `len` bytes from `ptr` into a stack buffer, NUL-terminate,
 * print, and return the count. Verifies the C side reads exactly `len`
 * bytes (not a strlen of a possibly-null-terminated input). */
int32_t mty_row12_echo(const char *ptr, size_t len) {
  char buf[256];
  size_t cap = sizeof(buf) - 1;
  size_t n = len < cap ? len : cap;
  if (n && ptr) {
    memcpy(buf, ptr, n);
  }
  buf[n] = '\0';
  printf("row12:echo='%s',len=%zu\n", buf, len);
  fflush(stdout);
  return (int32_t)len;
}

/* Empty / NULL ptr smoke: the Mighty side passes "" — len must be 0 and
 * the call must not segfault even if the ptr is whatever the codegen
 * picks (intern_string still returns a real symbol, but the C contract
 * is: do not deref unless len > 0). */
int32_t mty_row12_accept_empty(const char *ptr, size_t len) {
  (void)ptr;
  printf("row12:empty,len=%zu\n", len);
  fflush(stdout);
  return (int32_t)len;
}

/* Non-ASCII (multi-byte UTF-8) round trip — confirm len is the BYTE
 * count, not the codepoint count. "héllo" = 6 bytes (é = 0xc3 0xa9). */
int32_t mty_row12_utf8(const char *ptr, size_t len) {
  char buf[256];
  size_t cap = sizeof(buf) - 1;
  size_t n = len < cap ? len : cap;
  if (n && ptr) {
    memcpy(buf, ptr, n);
  }
  buf[n] = '\0';
  printf("row12:utf8='%s',bytes=%zu\n", buf, len);
  fflush(stdout);
  return (int32_t)len;
}
