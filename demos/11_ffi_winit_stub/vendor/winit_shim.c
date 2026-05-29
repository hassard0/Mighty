/*
 * Demo 11 — winit shim.
 *
 * No-op implementations of the four entry points the demo's
 * extern c block declares. A real binding would forward to winit's
 * C-ABI surface (`Window::new`, `EventLoop::run`, etc.). The shim
 * stays here so the smoke test is hermetic and works on every host
 * we target without a system winit install.
 *
 * Build into a static archive via:
 *   cc -c vendor/winit_shim.c -o vendor/winit_shim.o
 *   ar rcs vendor/libwinit_shim.a vendor/winit_shim.o
 *
 * The matching `[[extern_lib]]` in mighty.toml then points at
 * `vendor/libwinit_shim.a` and the cranelift backend declares each
 * extern fn with `Linkage::Import` so the system linker resolves
 * them from the archive.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

int32_t winit_shim_init(void) {
  fputs("winit_shim_init: stub ok\n", stderr);
  return 0;
}

int32_t winit_shim_open_window(int32_t w, int32_t h,
                               const unsigned char *title_ptr,
                               size_t title_len) {
  (void)title_ptr; (void)title_len;
  fprintf(stderr, "winit_shim_open_window: %dx%d stub ok\n", w, h);
  return 1; /* synthetic window id */
}

int32_t winit_shim_run_event_loop_once(void) {
  /* Real binding would poll the platform event queue. Stub returns
   * 0 to mean "no events, fall through to shutdown". */
  return 0;
}

int32_t winit_shim_shutdown(void) {
  fputs("winit_shim_shutdown: stub ok\n", stderr);
  return 0;
}
