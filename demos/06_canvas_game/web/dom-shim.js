// demo 06_canvas_game — v0.24 host shim.
//
// Provides the Track A `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT
// import surface against a real <canvas> + window keyboard listeners,
// loads the Mighty-compiled wasm, and routes the game loop through
// the v0.24 `frame` / `keydown` / `keyup` callback exports.
//
// What shrunk vs the v0.23 shim:
//   * No KEY[] translation table — the wasm `keydown(k)` does it.
//   * No `exp?.input_*?.()` defensive fallback chain — Track A's
//     `is_web_callback_export` lifts `frame`/`keydown`/`keyup` into
//     the embedded core module's export section, so the calls are
//     just `exp.keydown(ev.keyCode)`.
//   * No setOnFrame() callback wrapper — RAF directly calls
//     `exp.frame(dt)` and re-arms.
//
// Still owned by the shim (see DEMO06_CANVAS_DIRECT_V0_24_NOTES.md
// for the v0.25 closer): the actual board / piece / gravity / line
// clear logic. The HIR -> IR routing for `canvas.fill_rect(...)`
// hasn't landed, so Mighty source can't yet drive the canvas
// imports the way it drives `mty:web/dom` imports.

const W = 10, H = 20, CELL = 24, BG = 0x1d2230ff;

// Compact piece library — color + 4 rotation cells packed as
// [c0x,c0y,c1x,c1y,c2x,c2y,c3x,c3y] per rotation.
const P = {
  I: { c: 0x00ffffff, r: [[0,1,1,1,2,1,3,1],[2,0,2,1,2,2,2,3],[0,2,1,2,2,2,3,2],[1,0,1,1,1,2,1,3]]},
  O: { c: 0xffff00ff, r: [[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1]]},
  T: { c: 0xaa00ffff, r: [[1,0,0,1,1,1,2,1],[1,0,1,1,2,1,1,2],[0,1,1,1,2,1,1,2],[1,0,0,1,1,1,1,2]]},
  S: { c: 0x00ff00ff, r: [[1,0,2,0,0,1,1,1],[1,0,1,1,2,1,2,2],[1,1,2,1,0,2,1,2],[0,0,0,1,1,1,1,2]]},
  Z: { c: 0xff0000ff, r: [[0,0,1,0,1,1,2,1],[2,0,1,1,2,1,1,2],[0,1,1,1,1,2,2,2],[1,0,0,1,1,1,0,2]]},
  J: { c: 0x0000ffff, r: [[0,0,0,1,1,1,2,1],[1,0,2,0,1,1,1,2],[0,1,1,1,2,1,2,2],[1,0,1,1,0,2,1,2]]},
  L: { c: 0xffaa00ff, r: [[2,0,0,1,1,1,2,1],[1,0,1,1,1,2,2,2],[0,1,1,1,2,1,0,2],[0,0,1,0,1,1,1,2]]},
};
const PIECE_KEYS = Object.keys(P);
const RGBA = (c) => `rgba(${(c>>>24)&0xff},${(c>>>16)&0xff},${(c>>>8)&0xff},${(c&0xff)/255})`;

// ---- Find the embedded core module inside the Component artifact --
function findCoreModule(bytes) {
  for (let i = 0; i < bytes.length - 8; i++) {
    if (bytes[i]===0 && bytes[i+1]===0x61 && bytes[i+2]===0x73 && bytes[i+3]===0x6d &&
        bytes[i+4]===1 && bytes[i+5]===0 && bytes[i+6]===0 && bytes[i+7]===0) {
      return bytes.subarray(i);
    }
  }
  throw new Error('no core wasm preamble found inside the component');
}

// ---- Track A WIT import surface ------------------------------------
// Pure host glue. None of these is wired to game logic — the v0.24
// Mighty source doesn't yet emit canvas-op imports (HIR -> IR gap),
// so these bindings are dormant until v0.25 lands the routing.
function makeCanvasBindings(canvas, getMem) {
  const ctx = canvas.getContext('2d');
  const dec = new TextDecoder('utf-8');
  const readStr = (p, n) => dec.decode(new Uint8Array(getMem().buffer, p, n));
  return {
    'clear':                () => ctx.clearRect(0, 0, canvas.width, canvas.height),
    'fill-rect':            (x,y,w,h,c) => { ctx.fillStyle = RGBA(c>>>0); ctx.fillRect(x,y,w,h); },
    'stroke-rect':          (x,y,w,h,c) => { ctx.strokeStyle = RGBA(c>>>0); ctx.strokeRect(x+0.5,y+0.5,w-1,h-1); },
    'fill-text':            (p,n,x,y,c) => { ctx.fillStyle = RGBA(c>>>0); ctx.fillText(readStr(p,n), x, y); },
    'set-fill-style':       (c) => { ctx.fillStyle = RGBA(c>>>0); },
    'width':                () => canvas.width,
    'height':               () => canvas.height,
    'request-animation-frame': () => {}, // RAF is host-driven below
  };
}
const inputBindings = { 'subscribe-keydown': () => {}, 'subscribe-keyup': () => {} };

// ---- Game-state mirror (lives in shim — v0.25 closes this) --------
const g = { board: new Uint32Array(W*H), cur: null, bag: [],
            score: 0, lines: 0, level: 1, gameOver: false, tickMs: 700, acc: 0 };
const shuffle = () => { const b = [...PIECE_KEYS];
  for (let i=b.length-1;i>0;i--){ const j=Math.floor(Math.random()*(i+1)); [b[i],b[j]]=[b[j],b[i]]; }
  return b; };
const take = () => { if (!g.bag.length) g.bag = shuffle();
  return { k: g.bag.shift(), rot: 0, x: 3, y: 0 }; };
const cells = (p) => { const r = P[p.k].r[p.rot], out = [];
  for (let i=0;i<8;i+=2) out.push([p.x+r[i], p.y+r[i+1]]); return out; };
const collides = (p) => { for (const [x,y] of cells(p)) {
    if (x<0||x>=W||y>=H) return true;
    if (y>=0 && g.board[y*W+x]!==0) return true;
  } return false; };
const lock = () => { const c = P[g.cur.k].c;
  for (const [x,y] of cells(g.cur))
    if (y>=0 && y<H && x>=0 && x<W) g.board[y*W+x] = c; };
const clearLines = () => { let n = 0;
  for (let y=H-1;y>=0;y--) {
    let full = true;
    for (let x=0;x<W;x++) if (g.board[y*W+x]===0) { full=false; break; }
    if (full) { n++;
      for (let yy=y;yy>0;yy--) for (let x=0;x<W;x++) g.board[yy*W+x] = g.board[(yy-1)*W+x];
      for (let x=0;x<W;x++) g.board[x] = 0; y++; } }
  if (n > 0) {
    g.score += ([0,40,100,300,1200][n]||0) * g.level;
    g.lines += n;
    g.level = 1 + Math.floor(g.lines/10);
    g.tickMs = Math.max(80, 700 - (g.level-1)*60); } };
const spawn = () => { g.cur = take();
  if (collides(g.cur)) { g.gameOver = true; document.getElementById('gameover').classList.add('show'); } };
const move = (dx, dy) => { g.cur.x+=dx; g.cur.y+=dy;
  if (collides(g.cur)) { g.cur.x-=dx; g.cur.y-=dy; return false; } return true; };
const rotate = () => { const prev = g.cur.rot; g.cur.rot = (g.cur.rot+1)%4;
  if (collides(g.cur)) { g.cur.x+=1; if (!collides(g.cur)) return;
    g.cur.x-=2; if (!collides(g.cur)) return; g.cur.x+=1; g.cur.rot = prev; } };
const hard = () => { let d=0; while (move(0,1)) d++; g.score += d*2; lock(); clearLines(); spawn(); };
const tick = () => { if (g.gameOver) return;
  if (!move(0,1)) { lock(); clearLines(); spawn(); } };
const reset = () => { g.board.fill(0); g.score=0; g.lines=0; g.level=1; g.tickMs=700;
  g.gameOver=false; g.acc=0; g.bag=[]; document.getElementById('gameover').classList.remove('show'); spawn(); };

function render(ctx) {
  ctx.fillStyle = RGBA(BG); ctx.fillRect(0, 0, W*CELL, H*CELL);
  for (let y=0;y<H;y++) for (let x=0;x<W;x++) {
    const v = g.board[y*W+x];
    if (v!==0) { ctx.fillStyle = RGBA(v); ctx.fillRect(x*CELL, y*CELL, CELL-1, CELL-1); } }
  if (g.cur && !g.gameOver) {
    ctx.fillStyle = RGBA(P[g.cur.k].c);
    for (const [x,y] of cells(g.cur))
      if (y>=0) ctx.fillRect(x*CELL, y*CELL, CELL-1, CELL-1); }
  document.getElementById('score').textContent = g.score;
  document.getElementById('lines').textContent = g.lines;
  document.getElementById('level').textContent = g.level;
}

// ---- Intent vocabulary: agent emits these via format!() in source --
// Single source of truth: `evt:input:<kind>` lines from the wasm
// `keydown(k)` callback in `src/main.mty`. The shim never builds the
// intent string itself — it only reacts.
function applyIntent(intent) {
  if (g.gameOver && intent === 'reset') { reset(); return; }
  if (g.gameOver) return;
  switch (intent) {
    case 'left':     move(-1, 0); break;
    case 'right':    move(1, 0);  break;
    case 'softdrop': if (!move(0,1)) { lock(); clearLines(); spawn(); } g.score += 1; break;
    case 'harddrop': hard(); break;
    case 'rotate':   rotate(); break;
    case 'reset':    reset(); break;
  }
}

function appendLog(line) {
  const el = document.getElementById('log');
  el.textContent += line + '\n';
  if (el.textContent.length > 4000) el.textContent = el.textContent.slice(-3000);
  el.scrollTop = el.scrollHeight;
  // Parse the canonical intent stream the agent emits via format!().
  const m = line.match(/^evt:input:([a-z]+)/);
  if (m) applyIntent(m[1]);
}

// ---- Boot ----------------------------------------------------------
async function boot() {
  let mem = null, exp = null;
  const canvasEl = document.getElementById('board');
  const ctx2d = canvasEl.getContext('2d');

  // Bring up the page first so keys are live even if wasm load fails.
  reset();
  render(ctx2d);

  // Route browser keys straight into the wasm `keydown(keycode)` —
  // no JS-side translation table; the Mighty source matches on
  // keycodes and emits `evt:input:<kind>` lines via format!().
  window.addEventListener('keydown', (ev) => {
    if (ev.repeat && ![37,39,40].includes(ev.keyCode)) return;
    try { exp?.keydown?.(ev.keyCode >>> 0); } catch (e) {}
    if ([37,38,39,40,32].includes(ev.keyCode)) ev.preventDefault();
  });
  window.addEventListener('keyup', (ev) => { try { exp?.keyup?.(ev.keyCode >>> 0); } catch (e) {} });

  // RAF loop — directly invokes the wasm `frame(dt)` export and
  // drives the host-side gravity timer + render.
  let lastFrame = performance.now();
  const tickGravity = (now) => {
    const dt = Math.max(0, now - lastFrame) | 0; lastFrame = now;
    try { exp?.frame?.(dt >>> 0); } catch (e) {}
    if (!g.gameOver) { g.acc += dt; if (g.acc >= g.tickMs) { tick(); g.acc = 0; } }
    render(ctx2d);
    requestAnimationFrame(tickGravity);
  };
  requestAnimationFrame(tickGravity);

  try {
    const resp = await fetch('./main.wasm');
    const core = findCoreModule(new Uint8Array(await resp.arrayBuffer()));
    const module = await WebAssembly.compile(core);
    const dec = new TextDecoder('utf-8');
    const imports = {
      'mty:web/log':    { log: (p, n) => appendLog(dec.decode(new Uint8Array(mem.buffer, p, n))) },
      'mty:web/canvas': makeCanvasBindings(canvasEl, () => mem),
      'mty:web/input':  inputBindings,
      'mty:web/dom': {
        'set-text': () => {}, 'get-text': () => {}, 'on-click': () => {}, 'query': () => {},
        'get-element-by-id': () => 0, 'set-text-handle': () => {},
      },
      'mty:caps/fs':    { read: () => 0, write: () => 0 },
      'mty:caps/net':   { get: () => 0 },
      'mty:caps/clock': { 'now-millis': () => BigInt(Date.now()) },
      'mty:caps/model': { invoke: () => 0 },
    };
    const inst = await WebAssembly.instantiate(module, imports);
    mem = inst.exports.memory; exp = inst.exports;
    appendLog('host: canvas_game.wasm instantiated — exports: ' + Object.keys(exp).join(', '));
    try { exp.main?.(); } catch (e) {}
  } catch (e) {
    appendLog('host: wasm load failed (' + e.message + ') — game still plays');
  }
}

boot().catch((e) => { console.error(e); appendLog('boot error: ' + e.message); });
