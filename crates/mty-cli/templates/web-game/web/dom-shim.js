// {{NAME}} — JS host shim for the Mighty web-game template.
//
// This file is the "outside" half of the v0.22-era browser-game
// pattern (the same one `demos/05_notetris_web/web/dom-shim.js`
// uses). It:
//
//   1. fetches `/main.wasm` (served by `mty serve`),
//   2. extracts the embedded core module from the Component Model
//      envelope — browsers don't execute components natively yet,
//   3. instantiates with a `log` import that the Mighty agent uses
//      to emit `evt:...` lines,
//   4. wires keyboard events to the wasm-exported handler funcs,
//   5. mirrors the agent's logged events onto a <canvas>.
//
// When the canvas WIT lands (see `mty:web/canvas@0.1`) the guest
// will own the draws and this file shrinks to ~30 lines of input
// plumbing. Until then, the mirror here is intentionally tiny so
// the game plays end-to-end on day-one of a fresh scaffold.
//
// `mty serve --watch` also opens a websocket at `/_reload`; when
// the server pushes `reload`, we `location.reload()`. The dev
// loop on save: edit → server rebuilds → ws push → page reloads.

// ---- Canvas refs ----------------------------------------------------
const canvas = document.getElementById('board');
const ctx = canvas.getContext('2d');
const scoreEl = document.getElementById('score');
const logEl = document.getElementById('log');

// ---- Component-model loader (same trick as demos/05_notetris_web) --
async function loadWasm() {
  const resp = await fetch('./main.wasm');
  if (!resp.ok) throw new Error('failed to fetch /main.wasm: ' + resp.status);
  const buf = new Uint8Array(await resp.arrayBuffer());
  return findCoreModule(buf);
}

function findCoreModule(bytes) {
  // Components start with `\0asm\x0d\x00\x01\x00`; core modules
  // with `\0asm\x01\x00\x00\x00`. Browsers refuse the former, so
  // we scan for the latter and hand `WebAssembly.instantiate` the
  // sub-buffer starting at the first inner core module.
  for (let i = 0; i < bytes.length - 8; i++) {
    if (bytes[i] === 0x00 && bytes[i+1] === 0x61 &&
        bytes[i+2] === 0x73 && bytes[i+3] === 0x6d &&
        bytes[i+4] === 0x01 && bytes[i+5] === 0x00 &&
        bytes[i+6] === 0x00 && bytes[i+7] === 0x00) {
      return bytes.subarray(i);
    }
  }
  throw new Error('no core wasm preamble found inside the component');
}

// ---- Log stream + tiny event mirror --------------------------------
function appendLog(line) {
  if (!logEl) return;
  logEl.textContent += line + '\n';
  if (logEl.textContent.length > 4000) {
    logEl.textContent = logEl.textContent.slice(-3000);
  }
  logEl.scrollTop = logEl.scrollHeight;
}

// Per-game mirror state. Trivial bouncing dot driven by the agent's
// `evt:move:*` lines. Replace with your own draws as the game grows.
const W = canvas.width;
const H = canvas.height;
let player = { x: W / 2, y: H / 2 };
let score = 0;

function draw() {
  ctx.fillStyle = '#0b0d12';
  ctx.fillRect(0, 0, W, H);
  ctx.fillStyle = '#b6f';
  ctx.fillRect(player.x - 8, player.y - 8, 16, 16);
}

function handleEvent(kind) {
  switch (kind) {
    case 'move:left':  player.x = Math.max(8, player.x - 16); break;
    case 'move:right': player.x = Math.min(W - 8, player.x + 16); break;
    case 'move:up':    player.y = Math.max(8, player.y - 16); break;
    case 'move:down':  player.y = Math.min(H - 8, player.y + 16); break;
    case 'fire':       score += 1; scoreEl.textContent = score; break;
    case 'reset':      player = { x: W / 2, y: H / 2 }; score = 0; scoreEl.textContent = 0; break;
    default: break;
  }
  draw();
}

function logImport(msg) {
  appendLog(msg);
  if (typeof msg === 'string' && msg.startsWith('evt:')) {
    handleEvent(msg.slice(4));
  }
}

// ---- Decode the imported wasm string -------------------------------
// The Mighty wasm component lowers `log(msg: string)` as a host
// import taking `(ptr: i32, len: i32)` over the guest's linear
// memory. We read those bytes out as UTF-8 on each call.
function makeLogImport(getMemory) {
  return (ptr, len) => {
    const mem = getMemory();
    if (!mem) return;
    const bytes = new Uint8Array(mem.buffer, ptr, len);
    const msg = new TextDecoder('utf-8').decode(bytes);
    logImport(msg);
  };
}

// ---- Boot ----------------------------------------------------------
async function boot() {
  let instance;
  const memBox = { instance: null };
  const importObj = {
    env: { log: makeLogImport(() => memBox.instance && memBox.instance.exports.memory) },
    // Some Mighty-emitted modules import `log` from `mty` instead
    // of `env`; alias both so the template tolerates either.
    mty: { log: makeLogImport(() => memBox.instance && memBox.instance.exports.memory) },
  };
  try {
    const bytes = await loadWasm();
    const { instance: inst } = await WebAssembly.instantiate(bytes, importObj);
    instance = inst;
    memBox.instance = inst;
  } catch (e) {
    appendLog('[host] failed to load wasm: ' + e);
    return;
  }

  // Call `start()` once at boot if the guest exported one.
  if (typeof instance.exports.start === 'function') instance.exports.start();

  // Wire keyboard.
  window.addEventListener('keydown', (e) => {
    const ex = instance.exports;
    switch (e.key) {
      case 'ArrowLeft':  if (ex.move_left)  ex.move_left();  break;
      case 'ArrowRight': if (ex.move_right) ex.move_right(); break;
      case 'ArrowUp':    if (ex.move_up)    ex.move_up();    break;
      case 'ArrowDown':  if (ex.move_down)  ex.move_down();  break;
      case ' ':          if (ex.fire)       ex.fire();       e.preventDefault(); break;
      case 'r': case 'R': if (ex.reset)     ex.reset();      break;
      default: return;
    }
  });

  // Tick the agent on every animation frame.
  function frame() {
    if (instance.exports.tick) instance.exports.tick();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  draw();
}

// ---- `mty serve --watch` reload websocket --------------------------
// The server pushes the string "reload" whenever a rebuild
// finishes; we just call `location.reload()`. Connection failures
// are silent so `mty serve` (no --watch) doesn't spam errors.
function connectReload() {
  try {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/_reload`);
    ws.addEventListener('message', (ev) => {
      if (typeof ev.data === 'string' && ev.data.includes('reload')) {
        location.reload();
      }
    });
    ws.addEventListener('close', () => setTimeout(connectReload, 1000));
  } catch (e) { /* silent */ }
}

boot();
connectReload();
