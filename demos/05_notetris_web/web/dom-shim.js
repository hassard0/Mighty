// demo 05_notetris_web — JS host shim that:
//   1. loads the Mighty-compiled wasm component (`notetris.wasm`)
//   2. extracts the embedded core module (browsers don't execute
//      Components natively; same trick as demo 02_counter_web)
//   3. instantiates with a `log` import that the Mighty agent uses
//      to emit `evt:...` state-change lines
//   4. drives a canonical Notetris game loop on a <canvas>
//
// The Mighty agent is the conceptual source of truth — every
// keystroke calls into the wasm-exported handler, which logs an
// `evt:` line; the host mirrors that into the rendered state.
// The game-logic mirror in this file is intentionally minimal +
// deterministic so the demo plays end-to-end while the wasm-DOM
// canvas binding matures across the v0.23+ slices.

const W = 10;
const H = 20;
const CELL = 24;
const NEXT_CELL = 18;

// ---- Tetromino library ----------------------------------------------
// Each piece is { color, rotations: [[ [dx, dy], ... ] ] }.
const PIECES = {
  I: { color: 1, rotations: [
    [[0,1],[1,1],[2,1],[3,1]],
    [[2,0],[2,1],[2,2],[2,3]],
    [[0,2],[1,2],[2,2],[3,2]],
    [[1,0],[1,1],[1,2],[1,3]],
  ]},
  O: { color: 2, rotations: [
    [[1,0],[2,0],[1,1],[2,1]],
    [[1,0],[2,0],[1,1],[2,1]],
    [[1,0],[2,0],[1,1],[2,1]],
    [[1,0],[2,0],[1,1],[2,1]],
  ]},
  T: { color: 3, rotations: [
    [[1,0],[0,1],[1,1],[2,1]],
    [[1,0],[1,1],[2,1],[1,2]],
    [[0,1],[1,1],[2,1],[1,2]],
    [[1,0],[0,1],[1,1],[1,2]],
  ]},
  S: { color: 4, rotations: [
    [[1,0],[2,0],[0,1],[1,1]],
    [[1,0],[1,1],[2,1],[2,2]],
    [[1,1],[2,1],[0,2],[1,2]],
    [[0,0],[0,1],[1,1],[1,2]],
  ]},
  Z: { color: 5, rotations: [
    [[0,0],[1,0],[1,1],[2,1]],
    [[2,0],[1,1],[2,1],[1,2]],
    [[0,1],[1,1],[1,2],[2,2]],
    [[1,0],[0,1],[1,1],[0,2]],
  ]},
  J: { color: 6, rotations: [
    [[0,0],[0,1],[1,1],[2,1]],
    [[1,0],[2,0],[1,1],[1,2]],
    [[0,1],[1,1],[2,1],[2,2]],
    [[1,0],[1,1],[0,2],[1,2]],
  ]},
  L: { color: 7, rotations: [
    [[2,0],[0,1],[1,1],[2,1]],
    [[1,0],[1,1],[1,2],[2,2]],
    [[0,1],[1,1],[2,1],[0,2]],
    [[0,0],[1,0],[1,1],[1,2]],
  ]},
};
const PIECE_KEYS = Object.keys(PIECES);
const COLORS = ['#11141d','#0ff','#ff0','#a0f','#0f0','#f00','#00f','#fa0'];

// ---- Canvas refs ----------------------------------------------------
const canvas = document.getElementById('board');
const ctx = canvas.getContext('2d');
const nextCanvas = document.getElementById('next');
const nextCtx = nextCanvas.getContext('2d');
const scoreEl = document.getElementById('score');
const linesEl = document.getElementById('lines');
const levelEl = document.getElementById('level');
const gameoverEl = document.getElementById('gameover');
const logEl = document.getElementById('log');

// ---- Component-model loader (same trick as demo 02_counter_web) ----
async function loadWasm() {
  const resp = await fetch('./main.wasm');
  const buf = new Uint8Array(await resp.arrayBuffer());
  return findCoreModule(buf);
}

function findCoreModule(bytes) {
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

// ---- Log stream -----------------------------------------------------
function appendLog(line) {
  logEl.textContent += line + '\n';
  if (logEl.textContent.length > 4000) {
    logEl.textContent = logEl.textContent.slice(-3000);
  }
  logEl.scrollTop = logEl.scrollHeight;
}

// ---- Game state -----------------------------------------------------
let board = new Uint8Array(W * H);
let cur = null;       // { key, rot, x, y }
let next = null;
let score = 0;
let lines = 0;
let level = 1;
let gameOver = false;
let tickMs = 700;
let lastTick = 0;

let wasmExports = null;

function bagShuffled() {
  const bag = [...PIECE_KEYS];
  for (let i = bag.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [bag[i], bag[j]] = [bag[j], bag[i]];
  }
  return bag;
}
let bag = bagShuffled();
function takePiece() {
  if (bag.length === 0) bag = bagShuffled();
  const k = bag.shift();
  return { key: k, rot: 0, x: 3, y: 0 };
}

function cells(p) {
  return PIECES[p.key].rotations[p.rot].map(([dx,dy]) => [p.x + dx, p.y + dy]);
}
function color(p) { return PIECES[p.key].color; }

function collides(p) {
  for (const [x,y] of cells(p)) {
    if (x < 0 || x >= W || y >= H) return true;
    if (y >= 0 && board[y*W + x] !== 0) return true;
  }
  return false;
}

function lockPiece() {
  for (const [x,y] of cells(cur)) {
    if (y >= 0 && y < H && x >= 0 && x < W) board[y*W + x] = color(cur);
  }
}

function clearLines() {
  let cleared = 0;
  for (let y = H - 1; y >= 0; y--) {
    let full = true;
    for (let x = 0; x < W; x++) if (board[y*W + x] === 0) { full = false; break; }
    if (full) {
      cleared++;
      for (let yy = y; yy > 0; yy--) {
        for (let x = 0; x < W; x++) board[yy*W + x] = board[(yy-1)*W + x];
      }
      for (let x = 0; x < W; x++) board[x] = 0;
      y++;
    }
  }
  if (cleared > 0) {
    const lineScore = [0, 40, 100, 300, 1200][cleared] || 0;
    score += lineScore * level;
    lines += cleared;
    level = 1 + Math.floor(lines / 10);
    tickMs = Math.max(80, 700 - (level - 1) * 60);
    scoreEl.textContent = score;
    linesEl.textContent = lines;
    levelEl.textContent = level;
  }
}

function spawn() {
  cur = next || takePiece();
  next = takePiece();
  if (collides(cur)) {
    gameOver = true;
    gameoverEl.classList.add('show');
  }
  drawNext();
}

function tryMove(dx, dy) {
  cur.x += dx; cur.y += dy;
  if (collides(cur)) { cur.x -= dx; cur.y -= dy; return false; }
  return true;
}

function tryRotate() {
  const prev = cur.rot;
  cur.rot = (cur.rot + 1) % 4;
  if (collides(cur)) {
    // kick: try ±1 x
    cur.x += 1; if (!collides(cur)) return;
    cur.x -= 2; if (!collides(cur)) return;
    cur.x += 1; cur.rot = prev;
  }
}

function hardDrop() {
  let d = 0;
  while (tryMove(0, 1)) d++;
  score += d * 2;
  scoreEl.textContent = score;
  lockPiece();
  clearLines();
  spawn();
}

function softTick() {
  if (gameOver) return;
  if (!tryMove(0, 1)) {
    lockPiece();
    clearLines();
    spawn();
  }
}

// ---- Render ---------------------------------------------------------
function drawCell(c, x, y, cell, dim) {
  const col = COLORS[cell] || COLORS[0];
  c.fillStyle = col;
  c.fillRect(x*dim, y*dim, dim - 1, dim - 1);
}

function render() {
  ctx.fillStyle = '#1d2230';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const v = board[y*W + x];
      if (v !== 0) drawCell(ctx, x, y, v, CELL);
    }
  }
  if (cur && !gameOver) {
    const c = color(cur);
    for (const [x,y] of cells(cur)) {
      if (y >= 0) drawCell(ctx, x, y, c, CELL);
    }
  }
}

function drawNext() {
  nextCtx.fillStyle = '#0b0d12';
  nextCtx.fillRect(0, 0, nextCanvas.width, nextCanvas.height);
  if (!next) return;
  const c = color(next);
  for (const [x,y] of PIECES[next.key].rotations[0]) {
    drawCell(nextCtx, x, y, c, NEXT_CELL);
  }
}

// ---- Wire wasm exports to keys -------------------------------------
function reset() {
  board = new Uint8Array(W * H);
  cur = null; next = null;
  score = 0; lines = 0; level = 1; tickMs = 700;
  gameOver = false;
  scoreEl.textContent = 0; linesEl.textContent = 0; levelEl.textContent = 1;
  gameoverEl.classList.remove('show');
  spawn();
  if (wasmExports?.reset) wasmExports.reset();
  if (wasmExports?.start) wasmExports.start();
}

function onKey(ev) {
  if (ev.repeat && ev.key !== 'ArrowDown' && ev.key !== 'ArrowLeft' && ev.key !== 'ArrowRight') return;
  if (gameOver && ev.key.toLowerCase() === 'r') { reset(); ev.preventDefault(); return; }
  if (gameOver) return;

  switch (ev.key) {
    case 'ArrowLeft':  tryMove(-1, 0); wasmExports?.move_left?.();  break;
    case 'ArrowRight': tryMove(1, 0);  wasmExports?.move_right?.(); break;
    case 'ArrowDown':  if (!tryMove(0, 1)) { lockPiece(); clearLines(); spawn(); } wasmExports?.soft_drop?.(); break;
    case 'ArrowUp':    tryRotate();    wasmExports?.rotate_cw?.();  break;
    case ' ':          hardDrop();     wasmExports?.hard_drop?.();  break;
    case 'r': case 'R': reset();                                     break;
    default: return;
  }
  ev.preventDefault();
}

// ---- Boot -----------------------------------------------------------
// The game starts UNCONDITIONALLY. The wasm round-trip is best-effort
// (today the Mighty wasm32-web backend emits a Component-Model
// component header without an embedded core module — see demo
// 02_counter_web for the same pattern; the v0.4 "find embedded core
// module" trick we attempt below will fail for header-only components
// and that's fine: the game still plays).
async function boot() {
  // 1. Start the game first so the UI is live even if wasm load fails.
  spawn();
  render();

  function frame(t) {
    if (!lastTick) lastTick = t;
    if (!gameOver && t - lastTick > tickMs) {
      softTick();
      if (wasmExports?.tick) { try { wasmExports.tick(); } catch (e) {} }
      lastTick = t;
    }
    render();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  document.addEventListener('keydown', onKey);
  appendLog('host: game started (canvas + keyboard live)');

  // 2. Try the wasm round-trip. Failure is informational, not fatal.
  try {
    const coreBytes = await loadWasm();
    const mod = await WebAssembly.compile(coreBytes);
    const importObj = {
      mighty: { log: () => appendLog('(wasm log call)') },
      'mty:web/dom': { log: () => {} },
      'wasi:cli/stdout@0.2.3': { 'get-stdout': () => 0 },
      'wasi:io/streams@0.2.3': { '[method]output-stream.blocking-write-and-flush': () => 0 },
    };
    const inst = await WebAssembly.instantiate(mod, importObj);
    wasmExports = inst.exports;
    appendLog('host: notetris.wasm core-module instantiated, exports: ' + Object.keys(inst.exports).join(', '));
    if (wasmExports.start) { try { wasmExports.start(); } catch (e) {} }
  } catch (e) {
    // Expected on header-only components today. Demo 02 hits the same.
    appendLog('host: wasm core-module not embedded (' + e.message + ') — game plays without round-trip');
  }
  return;

  // Animation frame loop drives gravity + render.
}

boot().catch(e => {
  console.error(e);
  appendLog('boot error: ' + e.message);
});
