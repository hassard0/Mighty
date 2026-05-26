/* C harness for native_abi/02_string_return.
 *
 * The Mighty fn `_greeting` returns a Str. Under the cabi_realloc
 * convention the emitted shape is:
 *   typedef struct { uint8_t *ptr; size_t len; } mty_str_t;
 *   mty_str_t _greeting(void);
 *
 * The harness prints the returned bytes and exits 0 on success.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct {
  uint8_t *ptr;
  size_t len;
} mty_str_t;

extern mty_str_t _greeting(void);

int main(void) {
  mty_str_t s = _greeting();
  if (s.len != 12) {
    return 1;
  }
  if (memcmp(s.ptr, "hello, world", 12) != 0) {
    return 2;
  }
  return 0;
}
