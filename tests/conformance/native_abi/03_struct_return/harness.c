/* C harness for native_abi/03_struct_return.
 *
 * The Mighty fn `_origin` returns a Point. The C shape is:
 *   typedef struct { int32_t x; int32_t y; } mty_point_t;
 *   mty_point_t _origin(void);
 *
 * Exits 0 when x == 3 && y == 4.
 */
#include <stdint.h>

typedef struct {
  int32_t x;
  int32_t y;
} mty_point_t;

extern mty_point_t _origin(void);

int main(void) {
  mty_point_t p = _origin();
  if (p.x != 3) return 1;
  if (p.y != 4) return 2;
  return 0;
}
