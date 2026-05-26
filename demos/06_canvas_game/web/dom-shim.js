// demo 06_canvas_game — v0.23 host shim.
//
// Provides the Track A `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT
// import surface against a real <canvas> + window keyboard listeners,
// loads the Mighty-compiled wasm, and routes the game loop through
// those bindings.
//
// Today's language gap (see dev/history/notes/CANVAS_GAME_V0_23_NOTES.md):
// `mty-codegen-wasm` lowers `log()` and `mty:web/dom` calls today but
// not Mighty-side `canvas.fill_rect(...)` calls and not dynamic-string
// `format!()` calls. The Mighty agent therefore owns score / level /
// lines + the input-intent stream; the game-logic mirror below
// computes per-cell board state until v0.24 lifts those lowerings.
// When that lands the mirror drops out and this file shrinks to the
// `bindings` object below (~50 lines).

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

// ---- Track A WIT import surface --------------------------------------
const RGBA = (c) =>
  `rgba(${(c>>>24)&0xff},${(c>>>16)&0xff},${(c>>>8)&0xff},${(c&0xff)/255})`;

function makeCanvas(canvas, getMem) {
  const ctx = canvas.getContext('2d');
  const dec = new TextDecoder('utf-8');
  let rafPending = false, onFrame = null;
  const readStr = (p, n) => dec.decode(new Uint8Array(getMem().buffer, p, n));
  const bindings = {
    'clear':                () => ctx.clearRect(0, 0, canvas.width, canvas.height),
    'fill-rect':            (x,y,w,h,c) => { ctx.fillStyle = RGBA(c>>>0); ctx.fillRect(x,y,w,h); },
    'stroke-rect':          (x,y,w,h,c) => { ctx.strokeStyle = RGBA(c>>>0); ctx.strokeRect(x+0.5,y+0.5,w-1,h-1); },
    'fill-text':            (p,n,x,y,c) => { ctx.fillStyle = RGBA(c>>>0); ctx.fillText(readStr(p,n), x, y); },
    'set-fill-style':       (c) => { ctx.fillStyle = RGBA(c>>>0); },
    'width':                () => canvas.width,
    'height':               () => canvas.height,
    'request-animation-frame': () => {
      if (rafPending) return; rafPending = true;
      const t0 = performance.now();
      requestAnimationFrame((t) => { rafPending = false; if (onFrame) onFrame(Math.max(0, t - t0) | 0); });
    },
  };
  return { bindings, setOnFrame(fn) { onFrame = fn; } };
}

function makeInput(onKd, onKu) {
  let attached = false;
  const attach = () => { if (attached) return; attached = true;
    window.addEventListener('keydown', (ev) => onKd && onKd(ev));
    window.addEventListener('keyup',   (ev) => onKu && onKu(ev));
  };
  return {
    'subscribe-keydown': attach,
    'subscribe-keyup':   attach,
  };
}

// ---- Embedded core module locator (Track B) --------------------------
function findCoreModule(bytes) {
  for (let i = 0; i < bytes.length - 8; i++) {
    if (bytes[i]===0 && bytes[i+1]===0x61 && bytes[i+2]===0x73 && bytes[i+3]===0x6d &&
        bytes[i+4]===1 && bytes[i+5]===0 && bytes[i+6]===0 && bytes[i+7]===0) {
      return bytes.subarray(i);
    }
  }
  throw new Error('no core wasm preamble found inside the component');
}

// ---- Game logic mirror (deletes when v0.24 lands canvas lowering) ----
const g = { board: new Uint32Array(W*H), cur: null, bag: [],
            score: 0, lines: 0, level: 1, gameOver: false, tickMs: 700, acc: 0 };

const shuffle = () => { const b = [...PIECE_KEYS];
  for (let i=b.length-1;i>0;i--){ const j=Math.floor(Math.random()*(i+1)); [b[i],b[j]]=[b[j],b[i]]; }
  return b; };
const take = () => { if (!g.bag.length) g.bag = shuffle();
  return { k: g.bag.shift(), rot: 0, x: 3, y: 0 }; };
const cells = (p) => { const r = P[p.k].r[p.rot], out = [];
  for (let i=0;i<8;i+=2) out.push([p.x+r[i], p.y+r[i+1]]); return out; };
const collides = (p) => {
  for (const [x,y] of cells(p)) {
    if (x<0||x>=W||y>=H) return true;
    if (y>=0 && g.board[y*W+x]!==0) return true;
  } return false;
};
const lock = () => { const c = P[g.cur.k].c;
  for (const [x,y] of cells(g.cur))
    if (y>=0 && y<H && x>=0 && x<W) g.board[y*W+x] = c;
};
const clearLines = () => { let n = 0;
  for (let y=H-1;y>=0;y--) {
    let full = true;
    for (let x=0;x<W;x++) if (g.board[y*W+x]===0) { full=false; break; }
    if (full) { n++;
      for (let yy=y;yy>0;yy--) for (let x=0;x<W;x++) g.board[yy*W+x] = g.board[(yy-1)*W+x];
      for (let x=0;x<W;x++) g.board[x] = 0; y++;
    }
  }
  if (n > 0) {
    g.score += ([0,40,100,300,1200][n]||0) * g.level;
    g.lines += n;
    g.level = 1 + Math.floor(g.lines/10);
    g.tickMs = Math.max(80, 700 - (g.level-1)*60);
  }
};
const spawn = () => { g.cur = take();
  if (collides(g.cur)) { g.gameOver = true; document.getElementById('gameover').classList.add('show'); }
};
const move = (dx, dy) => { g.cur.x+=dx; g.cur.y+=dy;
  if (collides(g.cur)) { g.cur.x-=dx; g.cur.y-=dy; return false; } return true; };
const rotate = () => { const prev = g.cur.rot; g.cur.rot = (g.cur.rot+1)%4;
  if (collides(g.cur)) { g.cur.x+=1; if (!collides(g.cur)) return;
    g.cur.x-=2; if (!collides(g.cur)) return; g.cur.x+=1; g.cur.rot = prev; }
};
const hard = () => { let d=0; while (move(0,1)) d++; g.score += d*2; lock(); clearLines(); spawn(); };
const tick = () => { if (g.gameOver) return;
  if (!move(0,1)) { lock(); clearLines(); spawn(); } };
const reset = () => { g.board.fill(0); g.score=0; g.lines=0; g.level=1; g.tickMs=700;
  g.gameOver=false; g.acc=0; g.bag=[]; document.getElementById('gameover').classList.remove('show'); spawn(); };

// ---- Renderer routes through Track A canvas bindings ----------------
function render(cv) {
  const b = cv.bindings;
  b['fill-rect'](0, 0, W*CELL, H*CELL, BG);
  for (let y=0;y<H;y++) for (let x=0;x<W;x++) {
    const v = g.board[y*W+x];
    if (v!==0) b['fill-rect'](x*CELL, y*CELL, CELL-1, CELL-1, v);
  }
  if (g.cur && !g.gameOver) {
    const c = P[g.cur.k].c;
    for (const [x,y] of cells(g.cur))
      if (y>=0) b['fill-rect'](x*CELL, y*CELL, CELL-1, CELL-1, c);
  }
  document.getElementById('score').textContent = g.score;
  document.getElementById('lines').textContent = g.lines;
  document.getElementById('level').textContent = g.level;
}

// ---- Input -----------------------------------------------------------
const KEY = { 'ArrowLeft':'left','ArrowRight':'right','ArrowDown':'softdrop',
              'ArrowUp':'rotate',' ':'harddrop','r':'reset','R':'reset' };

function appendLog(line) {
  const el = document.getElementById('log');
  el.textContent += line + '\n';
  if (el.textContent.length > 4000) el.textContent = el.textContent.slice(-3000);
  el.scrollTop = el.scrollHeight;
}

// ---- Boot ------------------------------------------------------------
async function boot() {
  let mem = null, exp = null;
  const getMem = () => mem;
  const canvasEl = document.getElementById('board');

  const dispatch = (intent) => {
    if (g.gameOver && intent === 'reset') { reset(); exp?.input_reset?.(); return; }
    if (g.gameOver) return;
    switch (intent) {
      case 'left':     move(-1, 0); exp?.input_left?.(); break;
      case 'right':    move(1, 0);  exp?.input_right?.(); break;
      case 'softdrop': if (!move(0,1)) { lock(); clearLines(); spawn(); } exp?.input_softdrop?.(); break;
      case 'rotate':   rotate();    exp?.input_rotate?.(); break;
      case 'harddrop': hard();      exp?.input_harddrop?.(); break;
      case 'reset':    reset();     exp?.input_reset?.(); break;
    }
  };

  const onKd = (ev) => {
    const intent = KEY[ev.key]; if (!intent) return;
    if (ev.repeat && !['left','right','softdrop'].includes(intent)) return;
    dispatch(intent);
    try { exp?.keydown?.(ev.key); } catch (e) {}
    ev.preventDefault();
  };
  const onKu = (ev) => { try { exp?.keyup?.(ev.key); } catch (e) {} };

  const cv = makeCanvas(canvasEl, getMem);
  const inp = makeInput(onKd, onKu);

  // Bring up the page first so keys are live even if wasm load fails.
  reset();
  render(cv);
  window.addEventListener('keydown', onKd);

  cv.setOnFrame((dt) => {
    if (!g.gameOver) {
      g.acc += dt;
      if (g.acc >= g.tickMs) { tick(); try { exp?.input_tick?.(); } catch (e) {} g.acc = 0; }
    }
    render(cv);
    cv.bindings['request-animation-frame']();
  });
  cv.bindings['request-animation-frame']();

  try {
    const resp = await fetch('./main.wasm');
    const core = findCoreModule(new Uint8Array(await resp.arrayBuffer()));
    const module = await WebAssembly.compile(core);
    const dec = new TextDecoder('utf-8');
    const imports = {
      'mty:web/log':    { log: (p, n) => appendLog(dec.decode(new Uint8Array(mem.buffer, p, n))) },
      'mty:web/canvas': cv.bindings,
      'mty:web/input':  inp,
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
    appendLog('host: canvas_game.wasm instantiated, exports: ' + Object.keys(exp).join(', '));
    try { exp.start?.(); } catch (e) {}
  } catch (e) {
    appendLog('host: wasm load failed (' + e.message + ') — game still plays');
  }
}

boot().catch((e) => { console.error(e); appendLog('boot error: ' + e.message); });
