//! `mty serve` — built-in dev server (v0.23 Track C).
//!
//! Reads `mighty.toml`, builds the user's package with
//! `--target wasm32-web`, then serves:
//!
//! * `GET /` → `web/index.html`
//! * `GET /<asset>` → `web/<asset>` (mime by extension)
//! * `GET /main.wasm` → the freshly-built artefact under
//!   `<out_dir>/<binary>.wasm`
//! * `GET /_reload` → websocket handshake (only when `--watch` is
//!   set). Each successful rebuild pushes the literal text
//!   `reload` to every connected client; the template's
//!   `dom-shim.js` calls `location.reload()` on receipt.
//!
//! With `--watch`, `notify` watches `src/` recursively; every change
//! debounces to a fresh `wasm32-web` build that hot-swaps the served
//! `main.wasm` and broadcasts a reload.
//!
//! See `dev/history/notes/MTY_SERVE_V0_23_NOTES.md` for the design
//! rationale and the ws handshake details.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};

use mty_codegen_wasm::WasmTarget;
use mty_driver::build::WasiPreview;
use mty_driver::{build_wasm, BuildOptions, BuildOutcome, BuildTarget};

/// CLI args for `mty serve`. Kept in its own struct so the test
/// harness can drive the server in-process without re-parsing
/// `clap` arguments.
#[derive(Debug, Clone)]
pub struct ServeArgs {
    /// Port to bind on `127.0.0.1`. Default 8000.
    pub port: u16,
    /// When true, file-watch `src/` and rebuild on each change.
    pub watch: bool,
    /// Package root (defaults to the current working directory).
    pub manifest_dir: Option<PathBuf>,
}

impl Default for ServeArgs {
    fn default() -> Self {
        Self {
            port: 8000,
            watch: false,
            manifest_dir: None,
        }
    }
}

/// In-memory mirror of the package's static assets + freshly-built
/// wasm. Routes read from this; rebuilds atomically replace the
/// wasm entry.
struct AppState {
    web_dir: PathBuf,
    wasm_path: RwLock<PathBuf>,
    reload_tx: broadcast::Sender<()>,
}

/// Public entry point invoked by `main.rs`. Spins up the server,
/// blocks until the OS signals shutdown (Ctrl-C) or the watcher
/// thread panics.
pub fn run(args: ServeArgs) -> i32 {
    let pkg_root = args
        .manifest_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let manifest = pkg_root.join("mighty.toml");
    if !manifest.is_file() {
        eprintln!(
            "mty serve: no `mighty.toml` at {} (cd into a package first or pass --manifest-dir)",
            pkg_root.display()
        );
        return 2;
    }

    // Build once before the listener is bound so the user sees the
    // initial diagnostic if their package is broken. Failure here
    // is fatal (return 2 like every other build-fail CLI path).
    let initial_wasm = match build_once(&pkg_root) {
        Ok(p) => p,
        Err(BuildErr::Frontend) => return 1,
        Err(BuildErr::Backend(e)) => {
            eprintln!("build error: {e}");
            return 2;
        }
        Err(BuildErr::Io(e)) => {
            eprintln!("build error: {e}");
            return 2;
        }
    };

    let web_dir = pkg_root.join("web");
    if !web_dir.is_dir() {
        eprintln!(
            "mty serve: no `web/` directory at {}; this package isn't a web-game (try `mty new --template web-game ...`)",
            web_dir.display()
        );
        return 2;
    }

    // Tokio runtime — `mty-cli` is otherwise a sync binary, so we
    // build a dedicated multi-thread runtime here. Two worker
    // threads are enough for a single-tab dev loop; the user can
    // raise this later if it ever matters.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mty serve: failed to start tokio runtime: {e}");
            return 2;
        }
    };

    let (reload_tx, _) = broadcast::channel::<()>(8);
    let state = Arc::new(AppState {
        web_dir,
        wasm_path: RwLock::new(initial_wasm),
        reload_tx,
    });

    let result = rt.block_on(async move {
        // Spin up the file watcher *before* binding so a near-instant
        // edit doesn't race the first request.
        let watcher_handle = if args.watch {
            Some(spawn_watcher(pkg_root.clone(), state.clone()))
        } else {
            None
        };

        let addr: SocketAddr = ([127, 0, 0, 1], args.port).into();
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mty serve: bind {addr}: {e}");
                return 2;
            }
        };
        let bound = listener.local_addr().unwrap_or(addr);
        println!("mty serve: listening on http://{} ", bound);
        if args.watch {
            println!(
                "mty serve: watching {} for changes",
                pkg_root.join("src").display()
            );
        }

        accept_loop(listener, state).await;
        drop(watcher_handle);
        0
    });

    result
}

// ----------------------------------------------------------------
// Build
// ----------------------------------------------------------------

enum BuildErr {
    Frontend,
    Backend(String),
    Io(String),
}

/// One-shot `wasm32-web` build. Returns the path to the produced
/// `.wasm` artefact on success.
fn build_once(pkg_root: &Path) -> Result<PathBuf, BuildErr> {
    let src_path = pkg_root.join("src").join("main.mty");
    let src = std::fs::read_to_string(&src_path)
        .map_err(|e| BuildErr::Io(format!("read {}: {}", src_path.display(), e)))?;
    let source_id = src_path.display().to_string();

    let out_dir = pkg_root.join("target");
    let name = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();

    let opts = BuildOptions {
        target: BuildTarget::Wasm(WasmTarget::Web),
        mode: mty_codegen_cranelift::artifact::BuildMode::Debug,
        out_dir: out_dir.clone(),
        binary_name: name.clone(),
        no_component: false,
        wasi_preview: WasiPreview::default(),
        user_wit: None,
    };

    match build_wasm(src, source_id, &opts, WasmTarget::Web) {
        BuildOutcome::WasmOk(p) => Ok(p),
        BuildOutcome::FrontendError => Err(BuildErr::Frontend),
        BuildOutcome::BackendError(e) => Err(BuildErr::Backend(e)),
        // Native outcomes can't be returned from a wasm build, but
        // we map them defensively rather than panic.
        BuildOutcome::NativeOk(_) | BuildOutcome::NativeOkNoLinker(_) => {
            Err(BuildErr::Backend("unexpected native outcome".into()))
        }
    }
}

// ----------------------------------------------------------------
// Watcher
// ----------------------------------------------------------------

fn spawn_watcher(pkg_root: PathBuf, state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    use notify::{EventKind, RecursiveMode, Watcher};

    // Build the channel + watcher synchronously so we can surface
    // errors before the async task is spawned. The watcher needs to
    // outlive the spawned task so we move it in alongside `rx`.
    let src_dir = pkg_root.join("src");
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let watcher_res = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            // Filter for content changes; ignore Access/Other
            // events that fire on mere reads.
            if matches!(
                ev.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                let _ = tx.send(());
            }
        }
    });
    let mut watcher = match watcher_res {
        Ok(w) => w,
        Err(e) => {
            eprintln!("mty serve: watcher init failed: {e}");
            return tokio::task::spawn(async {});
        }
    };
    if let Err(e) = watcher.watch(&src_dir, RecursiveMode::Recursive) {
        eprintln!("mty serve: watch({}) failed: {e}", src_dir.display());
        return tokio::task::spawn(async {});
    }

    // Bridge the blocking `std::sync::mpsc::Receiver` to a tokio
    // mpsc channel via one long-lived blocking task. This sidesteps
    // the Arc/Mutex ping-pong that `recv()`'s by-value taking would
    // otherwise force on the async side.
    let (atx, mut arx) = tokio::sync::mpsc::channel::<()>(16);
    std::thread::spawn(move || {
        while let Ok(()) = rx.recv() {
            if atx.blocking_send(()).is_err() {
                break;
            }
        }
    });

    tokio::task::spawn(async move {
        // Hold the watcher alive for the lifetime of this task; if
        // the user Ctrl-Cs, the runtime is dropped and the watcher
        // drops with it.
        let _watcher_guard = watcher;
        loop {
            // Wait for the next change event.
            if arx.recv().await.is_none() {
                return;
            }
            // Debounce: drain events that arrive within 200ms of
            // each other. A single editor save can fire Create +
            // Modify + Modify in quick succession.
            tokio::time::sleep(Duration::from_millis(200)).await;
            while arx.try_recv().is_ok() {}

            println!("mty serve: change detected, rebuilding");
            match build_once(&pkg_root) {
                Ok(new_path) => {
                    *state.wasm_path.write().await = new_path;
                    let _ = state.reload_tx.send(());
                    println!("mty serve: rebuild ok");
                }
                Err(BuildErr::Frontend) => {
                    eprintln!("mty serve: rebuild failed (frontend errors above)");
                }
                Err(BuildErr::Backend(e)) => {
                    eprintln!("mty serve: rebuild failed: {e}");
                }
                Err(BuildErr::Io(e)) => {
                    eprintln!("mty serve: rebuild failed: {e}");
                }
            }
        }
    })
}

// ----------------------------------------------------------------
// HTTP accept loop
// ----------------------------------------------------------------

async fn accept_loop(listener: tokio::net::TcpListener, state: Arc<AppState>) {
    use http_body_util::Full;
    use hyper::body::{Bytes, Incoming};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request as HReq, Response as HResp};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("mty serve: accept: {e}");
                return;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = service_fn(move |req: HReq<Incoming>| {
                let state = state.clone();
                async move {
                    let resp = handle(state, req).await;
                    Ok::<HResp<Full<Bytes>>, Infallible>(resp)
                }
            });
            let _ = http1::Builder::new()
                .serve_connection(io, svc)
                .with_upgrades()
                .await;
        });
    }
}

async fn handle(
    state: Arc<AppState>,
    req: hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::Full;
    use hyper::body::Bytes;

    let path = req.uri().path().to_string();

    // Websocket upgrade for the reload channel. We hand-roll the
    // RFC 6455 opening handshake + a one-message reload frame so we
    // don't pull `tokio-tungstenite` into the workspace just for
    // this. See MTY_SERVE_V0_23_NOTES.md §"Reload websocket".
    if path == "/_reload" {
        return handle_reload_ws(state, req).await;
    }

    // `/main.wasm` → the freshly-built artefact.
    if path == "/main.wasm" {
        let p = state.wasm_path.read().await.clone();
        return match tokio::fs::read(&p).await {
            Ok(bytes) => hyper::Response::builder()
                .status(200)
                .header("content-type", "application/wasm")
                .header("cache-control", "no-cache")
                .body(Full::new(Bytes::from(bytes)))
                .expect("response builds"),
            Err(_) => not_found(),
        };
    }

    // Static assets out of `web/`. `/` → `index.html`.
    let rel = if path == "/" {
        "index.html".to_string()
    } else {
        // Strip leading `/` and normalise `..` segments away so a
        // hostile client can't escape `web/`.
        let raw = path.trim_start_matches('/').to_string();
        if raw.contains("..") {
            return forbidden();
        }
        raw
    };
    let file = state.web_dir.join(&rel);

    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let mime = mime_for(&rel);
            hyper::Response::builder()
                .status(200)
                .header("content-type", mime)
                .body(Full::new(Bytes::from(bytes)))
                .expect("response builds")
        }
        Err(_) => not_found(),
    }
}

fn not_found() -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::Full;
    use hyper::body::Bytes;
    hyper::Response::builder()
        .status(404)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"404 not found")))
        .expect("404 builds")
}

fn forbidden() -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::Full;
    use hyper::body::Bytes;
    hyper::Response::builder()
        .status(403)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"403 forbidden")))
        .expect("403 builds")
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ----------------------------------------------------------------
// Hand-rolled websocket for `/_reload`
// ----------------------------------------------------------------

/// Compute the `Sec-WebSocket-Accept` header per RFC 6455 §4.2.2:
/// base64( SHA-1( client_key ++ MAGIC ) ).
///
/// Hand-rolled SHA-1 + base64 because pulling a crypto crate just
/// for one hash per page load is overkill — the dev-server isn't a
/// production WebSocket endpoint.
fn ws_accept(key: &str) -> String {
    const MAGIC: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut bytes = Vec::with_capacity(key.len() + MAGIC.len());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(MAGIC.as_bytes());
    let h = sha1_inline(&bytes);
    base64_encode(&h)
}

/// Tiny SHA-1 implementation — single fn, ~50 lines. We only need
/// one hash per ws handshake (~once per page load) so performance
/// doesn't matter. Test-covered via the known-answer in tests below.
fn sha1_inline(msg: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xefcd_ab89;
    let mut h2: u32 = 0x98ba_dcfe;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xc3d2_e1f0;

    // Pre-processing: append 0x80, zeroes, then 64-bit big-endian
    // length in bits.
    let bit_len = (msg.len() as u64) * 8;
    let mut padded = msg.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, slot) in w.iter_mut().take(16).enumerate() {
            *slot = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = if i < 20 {
                ((b & c) | ((!b) & d), 0x5a82_7999)
            } else if i < 40 {
                (b ^ c ^ d, 0x6ed9_eba1)
            } else if i < 60 {
                ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc)
            } else {
                (b ^ c ^ d, 0xca62_c1d6)
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, v) in [h0, h1, h2, h3, h4].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Tiny base64 encoder (standard alphabet, with `=` padding).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let chunks = bytes.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n: u32 = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHA[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHA[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHA[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

async fn handle_reload_ws(
    state: Arc<AppState>,
    req: hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::Full;
    use hyper::body::Bytes;

    // Pull the client's `Sec-WebSocket-Key` header.
    let Some(key) = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
    else {
        return hyper::Response::builder()
            .status(400)
            .body(Full::new(Bytes::from_static(b"missing Sec-WebSocket-Key")))
            .expect("400 builds");
    };

    let accept = ws_accept(&key);
    let mut rx = state.reload_tx.subscribe();

    // Spawn the post-upgrade task that pushes a "reload" frame on
    // every broadcast.
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                use hyper_util::rt::TokioIo;
                use tokio::io::AsyncWriteExt;
                let mut io = TokioIo::new(upgraded);
                while rx.recv().await.is_ok() {
                    // Build a server→client text frame containing
                    // "reload". RFC 6455 §5.2.
                    let payload = b"reload";
                    let mut frame = Vec::with_capacity(2 + payload.len());
                    // FIN=1, opcode=1 (text).
                    frame.push(0x81);
                    // Mask bit = 0 (server frames are unmasked) +
                    // 7-bit length (payload < 126).
                    frame.push(payload.len() as u8);
                    frame.extend_from_slice(payload);
                    if io.write_all(&frame).await.is_err() {
                        break;
                    }
                }
            }
            Err(_) => {
                // Client dropped before upgrade completed; nothing
                // to do.
            }
        }
    });

    hyper::Response::builder()
        .status(101)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", accept)
        .body(Full::new(Bytes::new()))
        .expect("101 builds")
}

// ----------------------------------------------------------------
// Tests — unit tests live here; cross-process integration tests
// live in `tests/cmd_serve.rs`.
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_for_known_exts() {
        assert_eq!(mime_for("foo.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("foo.js"), "application/javascript; charset=utf-8");
        assert_eq!(mime_for("foo.wasm"), "application/wasm");
        assert_eq!(mime_for("foo.bin"), "application/octet-stream");
    }

    #[test]
    fn sha1_known_answer() {
        // RFC 3174 test vector "abc".
        let h = sha1_inline(b"abc");
        assert_eq!(
            h,
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
            ]
        );
    }

    #[test]
    fn base64_roundtrip_known() {
        // RFC 4648 examples.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn ws_accept_rfc6455_example() {
        // RFC 6455 §1.3 worked example.
        let got = ws_accept("dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(got, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
