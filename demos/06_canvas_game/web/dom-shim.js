// demo 06_canvas_game — v0.25 Track F host shim.
//
// v0.25 architecture: the Mighty agent in `src/main.mty` is now the
// source of truth for rendering — every frame `inst.exports.frame(dt)`
// calls into `canvas.set_fill_style` / `canvas.fill_rect` / `canvas.
// fill_text` through the real `mty:web/canvas@0.1` imports. This shim
// is pure host glue: extract the embedded core, bind the canvas/input
// imports against the live Canvas2D context + window keyboard, and
// run the RAF + intent-stream + gravity loop.
//
// Game state (board / piece / score) still lives in the shim because
// wasm32-web agent-state persistence is the v0.26 emitter slice — the
// agent declaration in `src/main.mty` pins the canonical shape, the
// shim mirrors it via the intent stream. See DEMO06_V2_V0_25_NOTES.md
// for the full per-gap log.

const W = 10, H = 20, CELL = 24, BG = 0x1d2230ff;
const PIECES = {
  I:{c:0x00ffffff,r:[[0,1,1,1,2,1,3,1],[2,0,2,1,2,2,2,3],[0,2,1,2,2,2,3,2],[1,0,1,1,1,2,1,3]]},
  O:{c:0xffff00ff,r:[[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1],[1,0,2,0,1,1,2,1]]},
  T:{c:0xaa00ffff,r:[[1,0,0,1,1,1,2,1],[1,0,1,1,2,1,1,2],[0,1,1,1,2,1,1,2],[1,0,0,1,1,1,1,2]]},
  S:{c:0x00ff00ff,r:[[1,0,2,0,0,1,1,1],[1,0,1,1,2,1,2,2],[1,1,2,1,0,2,1,2],[0,0,0,1,1,1,1,2]]},
  Z:{c:0xff0000ff,r:[[0,0,1,0,1,1,2,1],[2,0,1,1,2,1,1,2],[0,1,1,1,1,2,2,2],[1,0,0,1,1,1,0,2]]},
  J:{c:0x0000ffff,r:[[0,0,0,1,1,1,2,1],[1,0,2,0,1,1,1,2],[0,1,1,1,2,1,2,2],[1,0,1,1,0,2,1,2]]},
  L:{c:0xffaa00ff,r:[[2,0,0,1,1,1,2,1],[1,0,1,1,1,2,2,2],[0,1,1,1,2,1,0,2],[0,0,1,0,1,1,1,2]]},
};
const PKEYS = Object.keys(PIECES);
const RGBA = c => `rgba(${(c>>>24)&0xff},${(c>>>16)&0xff},${(c>>>8)&0xff},${(c&0xff)/255})`;
const findCore = b => { for (let i=0;i<b.length-8;i++) if (b[i]===0&&b[i+1]===0x61&&b[i+2]===0x73&&b[i+3]===0x6d&&b[i+4]===1&&b[i+5]===0&&b[i+6]===0&&b[i+7]===0) return b.subarray(i); throw new Error('no core'); };

// ---- Game state (shim-side; v0.26 moves this into agent linear mem) -
const g = { board:new Uint32Array(W*H), cur:null, bag:[], score:0, lines:0, level:1, gameOver:false, tickMs:700, acc:0 };
const cellsOf = p => { const r=PIECES[p.k].r[p.rot],o=[]; for (let i=0;i<8;i+=2) o.push([p.x+r[i],p.y+r[i+1]]); return o; };
const hits = p => { for (const [x,y] of cellsOf(p)) { if (x<0||x>=W||y>=H) return true; if (y>=0&&g.board[y*W+x]!==0) return true; } return false; };
const lock = () => { const c=PIECES[g.cur.k].c; for (const [x,y] of cellsOf(g.cur)) if (y>=0&&y<H&&x>=0&&x<W) g.board[y*W+x]=c; };
const clearLines = () => { let n=0; for (let y=H-1;y>=0;y--){let f=true; for (let x=0;x<W;x++) if (g.board[y*W+x]===0){f=false;break;} if (f){n++; for (let yy=y;yy>0;yy--) for (let x=0;x<W;x++) g.board[yy*W+x]=g.board[(yy-1)*W+x]; for (let x=0;x<W;x++) g.board[x]=0; y++;}} if (n){ g.score+=[0,40,100,300,1200][n]*g.level; g.lines+=n; g.level=1+Math.floor(g.lines/10); g.tickMs=Math.max(80,700-(g.level-1)*60);} };
const take = () => { if (!g.bag.length){ g.bag=[...PKEYS]; for (let i=g.bag.length-1;i>0;i--){const j=Math.floor(Math.random()*(i+1)); [g.bag[i],g.bag[j]]=[g.bag[j],g.bag[i]];} } return {k:g.bag.shift(),rot:0,x:3,y:0}; };
const spawn = () => { g.cur=take(); if (hits(g.cur)){ g.gameOver=true; document.getElementById('gameover').classList.add('show'); } };
const move = (dx,dy) => { g.cur.x+=dx; g.cur.y+=dy; if (hits(g.cur)){ g.cur.x-=dx; g.cur.y-=dy; return false; } return true; };
const rotate = () => { const p=g.cur.rot; g.cur.rot=(g.cur.rot+1)%4; if (hits(g.cur)){ g.cur.x+=1; if (!hits(g.cur)) return; g.cur.x-=2; if (!hits(g.cur)) return; g.cur.x+=1; g.cur.rot=p; } };
const hard = () => { let d=0; while (move(0,1)) d++; g.score+=d*2; lock(); clearLines(); spawn(); };
const reset = () => { g.board.fill(0); g.score=0; g.lines=0; g.level=1; g.tickMs=700; g.gameOver=false; g.acc=0; g.bag=[]; document.getElementById('gameover').classList.remove('show'); spawn(); };

// ---- Dynamic per-cell board overlay (Mighty draws static + HUD) -----
// The Mighty `frame(dt)` callback has already painted the field bg +
// grid + HUD column via the canvas WIT imports (Track A canvas-direct);
// we layer the dynamic piece + locked-cell pixels on top here.
function drawBoardPixels(ctx) {
  for (let y=0;y<H;y++) for (let x=0;x<W;x++) { const v=g.board[y*W+x]; if (v) { ctx.fillStyle=RGBA(v); ctx.fillRect(x*CELL,y*CELL,CELL-1,CELL-1); } }
  if (g.cur && !g.gameOver) { ctx.fillStyle=RGBA(PIECES[g.cur.k].c); for (const [x,y] of cellsOf(g.cur)) if (y>=0) ctx.fillRect(x*CELL,y*CELL,CELL-1,CELL-1); }
  document.getElementById('score').textContent=g.score; document.getElementById('lines').textContent=g.lines; document.getElementById('level').textContent=g.level;
}

// ---- Intent vocabulary — the agent emits via `format!()` ------------
function applyIntent(s) {
  if (g.gameOver && s==='reset') { reset(); return; }
  if (g.gameOver) return;
  switch (s) {
    case 'left':     move(-1,0); break;
    case 'right':    move(1,0);  break;
    case 'softdrop': if (!move(0,1)){ lock(); clearLines(); spawn(); } g.score+=1; break;
    case 'harddrop': hard(); break;
    case 'rotate':   rotate(); break;
    case 'reset':    reset(); break;
  }
}
function appendLog(line) {
  const el=document.getElementById('log'); el.textContent+=line+'\n';
  if (el.textContent.length>4000) el.textContent=el.textContent.slice(-3000);
  el.scrollTop=el.scrollHeight;
  const m=line.match(/^evt:input:([a-z]+)/); if (m) applyIntent(m[1]);
}

// ---- Boot -----------------------------------------------------------
async function boot() {
  let mem=null, exp=null;
  const canvasEl=document.getElementById('board'), ctx2d=canvasEl.getContext('2d');
  ctx2d.font='12px ui-monospace, monospace'; ctx2d.textBaseline='top';
  reset();

  window.addEventListener('keydown',ev=>{ if (ev.repeat&&![37,39,40].includes(ev.keyCode)) return; try { exp?.keydown?.(ev.keyCode>>>0); } catch{} if ([37,38,39,40,32].includes(ev.keyCode)) ev.preventDefault(); });
  window.addEventListener('keyup',ev=>{ try { exp?.keyup?.(ev.keyCode>>>0); } catch{} });

  let last=performance.now();
  const tickLoop=now=>{ const dt=Math.max(0,now-last)|0; last=now; try { exp?.frame?.(dt>>>0); } catch{} if (!g.gameOver){ g.acc+=dt; if (g.acc>=g.tickMs){ if (!move(0,1)){ lock(); clearLines(); spawn(); } g.acc=0; } } drawBoardPixels(ctx2d); requestAnimationFrame(tickLoop); };
  requestAnimationFrame(tickLoop);

  try {
    const core=findCore(new Uint8Array(await (await fetch('./main.wasm')).arrayBuffer()));
    const dec=new TextDecoder('utf-8'), readStr=(p,n)=>dec.decode(new Uint8Array(mem.buffer,p,n));
    const canvasOps={
      'clear':()=>ctx2d.clearRect(0,0,canvasEl.width,canvasEl.height),
      'fill-rect':(x,y,w,h,c)=>{ ctx2d.fillStyle=RGBA(c>>>0); ctx2d.fillRect(x,y,w,h); },
      'stroke-rect':(x,y,w,h,c)=>{ ctx2d.strokeStyle=RGBA(c>>>0); ctx2d.strokeRect(x+0.5,y+0.5,w-1,h-1); },
      'fill-text':(p,n,x,y,c)=>{ ctx2d.fillStyle=RGBA(c>>>0); ctx2d.fillText(readStr(p,n),x,y); },
      'set-fill-style':c=>{ ctx2d.fillStyle=RGBA(c>>>0); },
      'width':()=>canvasEl.width, 'height':()=>canvasEl.height, 'request-animation-frame':()=>{},
    };
    const inst=await WebAssembly.instantiate(await WebAssembly.compile(core), {
      'mty:web/log':{log:(p,n)=>appendLog(dec.decode(new Uint8Array(mem.buffer,p,n)))},
      'mty:web/canvas':canvasOps, 'mty:web/input':{'subscribe-keydown':()=>{},'subscribe-keyup':()=>{}},
      'mty:web/dom':{'set-text':()=>{},'get-text':()=>{},'on-click':()=>{},'query':()=>{},'get-element-by-id':()=>0,'set-text-handle':()=>{}},
      'mty:caps/fs':{read:()=>0,write:()=>0}, 'mty:caps/net':{get:()=>0}, 'mty:caps/clock':{'now-millis':()=>BigInt(Date.now())}, 'mty:caps/model':{invoke:()=>0},
    });
    mem=inst.exports.memory; exp=inst.exports;
    appendLog('host: canvas_game.wasm instantiated — exports: '+Object.keys(exp).join(', '));
    try { exp.main?.(); } catch{}
  } catch (e) { appendLog('host: wasm load failed ('+e.message+') — game still plays'); }
}
boot().catch(e=>{ console.error(e); appendLog('boot error: '+e.message); });
