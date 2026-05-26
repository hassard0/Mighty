/* C harness for native_abi/04_callback.
 *
 * The Mighty fn takes a C function pointer and invokes it once.
 * The harness passes `triple` (multiplies by 3), Mighty calls it
 * with 21 -> 63, then doubles -> 126. Exits 126.
 */
#include <stdint.h>

typedef int32_t (*cb_t)(int32_t);

extern int32_t _twice_via_cb(cb_t cb);

static int32_t triple(int32_t x) { return x * 3; }

int main(void) {
  int32_t r = _twice_via_cb(triple);
  return (int)r;
}
