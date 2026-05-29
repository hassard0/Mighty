/*
 * Demo 11 — winit shim.
 *
 * No-op implementations of the extern entry points the demo's
 * `extern c` block declares. A real binding would forward to winit's
 * C-ABI surface (`Window::new`, `EventLoop::run`, etc.). The shim
 * stays here so the smoke test is hermetic and works on every host
 * we target without a system winit install.
 *
 * v0.37 Track T3 updated this set to exercise the three call-site
 * coercions:
 *   - `winit_shim_open_window(title: *U8, title_len: USize)` — title
 *     comes from a Mighty Str literal (Str→*U8 coercion).
 *   - `winit_shim_poll_event(out: *I32)` — out-param via `&mut local`.
 *   - `winit_shim_set_clip(r: Rect)` — struct literal at call site.
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
#include <string.h>

/* Matches Mighty `struct Rect { x: I32, y: I32, w: I32, h: I32 }`. */
typedef struct {
  int32_t x;
  int32_t y;
  int32_t w;
  int32_t h;
} Rect;

int32_t winit_shim_init(void) {
  fputs("winit_shim_init: stub ok\n", stderr);
  return 0;
}

int32_t winit_shim_open_window(int32_t w, int32_t h,
                               const unsigned char *title,
                               size_t title_len) {
  /* Mighty Strs are null-terminated by the cranelift backend's
   * intern_string, so printing through %s is safe even though the
   * Mighty side passes a *U8 + USize pair. The length is informational
   * (matches strlen on null-terminated input). */
  fprintf(stderr,
          "winit_shim_open_window: %dx%d title=\"%s\" (len=%zu) stub ok\n",
          w, h, title ? (const char *)title : "(null)", title_len);
  return 1; /* synthetic window id */
}

int32_t winit_shim_run_event_loop_once(void) {
  /* Real binding would poll the platform event queue. Stub returns
   * 0 to mean "no events, fall through to shutdown". */
  return 0;
}

int32_t winit_shim_poll_event(int32_t *out) {
  /* v0.37 demo of out-param. Writes a synthetic 1 into the caller's
   * slot so the Mighty side observes the write across the FFI
   * boundary. */
  if (out) *out = 1;
  fputs("winit_shim_poll_event: wrote 1 to out slot\n", stderr);
  return 0;
}

int32_t winit_shim_set_clip(Rect r) {
  fprintf(stderr,
          "winit_shim_set_clip: rect(%d,%d,%dx%d) stub ok\n",
          r.x, r.y, r.w, r.h);
  return 0;
}

int32_t winit_shim_shutdown(void) {
  fputs("winit_shim_shutdown: stub ok\n", stderr);
  return 0;
}
