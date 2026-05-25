// v0.8 — JS implementation of the `mty:web/dom` component interface
// declared in `crates/mty-codegen-wasm/src/wit.rs`.
//
// The Mighty compiler emits core-wasm imports under the canonical
// `mty:web/dom` module. v0.8 wires the canonical-ABI return-area
// for `string` and `option<string>` returns: the caller passes a
// `ret_area_ptr` (third arg) where the host writes the result
// (data_ptr, data_len) [+ optional disc byte for option<string>].
//
// Usage from the demo's index.html:
//
//   import { instantiateWithDomShim } from './dom-shim.js';
//   const inst = await instantiateWithDomShim(wasmBytes, {
//     log: (msg) => console.log(msg),
//   });
//   inst.exports.bump();

// Legacy "JS string table" pointer used by v0.5 `get-text` for back-
// compat tooling; preserved at 8192 with a 16-byte gap before the
// canonical-ABI return area.
const RETURN_BUF_OFFSET = 8192;
const RETURN_BUF_CAPACITY = 4096;
// v0.8 canonical-ABI return area for string / option<string> returns.
// Layout for `string`:           [data_ptr:i32 | data_len:i32]
// Layout for `option<string>`:   [disc:i32 | data_ptr:i32 | data_len:i32]
// `disc != 0` means `Some`; the shim writes 0 for `None`.
const DOM_RETURN_AREA = 8208;
// Heap pointer used by the shim to write result strings; grows by
// length each call (cycled when it approaches the return-buf cap).
let domHeapCursor = RETURN_BUF_OFFSET + 4;

/**
 * Build the `mty:web/dom` import object. Each function reads
 * `(ptr, len)` pairs from `wasmMem` using a fresh DataView (so it
 * stays correct after `memory.grow`).
 */
function makeDomImports(getMemory, onClickCallbacks) {
  const decoder = new TextDecoder('utf-8');
  const encoder = new TextEncoder();

  function readStr(ptr, len) {
    const mem = getMemory();
    const bytes = new Uint8Array(mem.buffer, ptr, len);
    return decoder.decode(bytes);
  }

  function writeReturnStr(s) {
    const mem = getMemory();
    const bytes = encoder.encode(s);
    if (bytes.length + 4 > RETURN_BUF_CAPACITY) {
      throw new Error(`dom return buffer overflow: ${bytes.length}`);
    }
    const view = new DataView(mem.buffer);
    // [len:i32 | bytes...] starting at RETURN_BUF_OFFSET (v0.5 legacy
    // tooling that still uses the u32-handle shape).
    view.setUint32(RETURN_BUF_OFFSET, bytes.length, true);
    new Uint8Array(mem.buffer, RETURN_BUF_OFFSET + 4, bytes.length).set(bytes);
    return RETURN_BUF_OFFSET;
  }

  // v0.8 — write `s` into a fresh slab and record (ptr, len) in the
  // canonical-ABI return area at offset `retArea` so the calling wasm
  // module can read the string back. Bumps `domHeapCursor` to give a
  // distinct address each call.
  function writeStringToReturnArea(retArea, s) {
    const mem = getMemory();
    const bytes = encoder.encode(s);
    const need = bytes.length + 4; // 4-byte alignment slack
    if (domHeapCursor + need > RETURN_BUF_OFFSET + RETURN_BUF_CAPACITY) {
      // Wrap around (the demo only retains one return string at a
      // time, so overwriting earlier data is safe).
      domHeapCursor = RETURN_BUF_OFFSET + 4;
    }
    const dataPtr = domHeapCursor;
    new Uint8Array(mem.buffer, dataPtr, bytes.length).set(bytes);
    domHeapCursor += need;
    const view = new DataView(mem.buffer);
    view.setUint32(retArea, dataPtr, true);
    view.setUint32(retArea + 4, bytes.length, true);
  }

  function writeNoneToReturnArea(retArea) {
    const mem = getMemory();
    const view = new DataView(mem.buffer);
    view.setUint32(retArea, 0, true);
    view.setUint32(retArea + 4, 0, true);
  }

  return {
    'set-text': (idPtr, idLen, textPtr, textLen) => {
      const id = readStr(idPtr, idLen);
      const text = readStr(textPtr, textLen);
      const el = document.getElementById(id);
      if (el) el.textContent = text;
    },
    // v0.8 canonical-ABI signature: (id_ptr, id_len, ret_area) -> ()
    'get-text': (idPtr, idLen, retArea) => {
      const id = readStr(idPtr, idLen);
      const el = document.getElementById(id);
      const s = el ? (el.textContent ?? '') : '';
      writeStringToReturnArea(retArea, s);
    },
    'on-click': (idPtr, idLen, tagPtr, tagLen) => {
      const id = readStr(idPtr, idLen);
      const tag = readStr(tagPtr, tagLen);
      const el = document.getElementById(id);
      if (!el) return;
      el.addEventListener('click', () => {
        const cb = onClickCallbacks[tag];
        if (typeof cb === 'function') cb();
      });
    },
    // v0.8 canonical-ABI signature: (sel_ptr, sel_len, ret_area) -> ()
    // Return-area layout: [disc:i32 | data_ptr:i32 | data_len:i32].
    // BUT we emit the same 8-byte (ptr,len) layout as get-text and use
    // a non-zero ptr as "Some" / zero ptr as "None" — keeps the wasm
    // import shape uniform.
    'query': (selPtr, selLen, retArea) => {
      const sel = readStr(selPtr, selLen);
      const el = document.querySelector(sel);
      if (!el) {
        writeNoneToReturnArea(retArea);
      } else {
        writeStringToReturnArea(retArea, el.id || '');
      }
    },
    // v0.4 back-compat: handle-based set-text remains available.
    'get-element-by-id': (idPtr, idLen) => {
      const id = readStr(idPtr, idLen);
      const el = document.getElementById(id);
      return el ? 1 : 0;
    },
    'set-text-handle': (_handle, textPtr, textLen) => {
      // We don't track handles in v0.5; the legacy interface is a
      // best-effort no-op for forward-compat with older demos.
      void readStr(textPtr, textLen);
    },
  };
}

/**
 * Instantiate `wasmBytes` (a *core* wasm module — strip the component
 * wrapper first if needed) with the DOM shim wired in.
 *
 * @param wasmBytes ArrayBuffer | Uint8Array — core wasm module
 * @param extraImports { log?: (msg: string) => void }
 * @param onClickCallbacks { [tag: string]: () => void }
 * @returns Promise<{ exports, memory }>
 */
export async function instantiateWithDomShim(
  wasmBytes,
  extraImports = {},
  onClickCallbacks = {},
) {
  let memory; // set during instantiate
  const getMemory = () => memory;
  const decoder = new TextDecoder('utf-8');

  const logFn = extraImports.log || ((msg) => console.log('[sd]', msg));
  const logImport = (ptr, len) => {
    const bytes = new Uint8Array(memory.buffer, ptr, len);
    logFn(decoder.decode(bytes));
  };

  const dom = makeDomImports(getMemory, onClickCallbacks);
  const imports = {
    'mty:web/log': { log: logImport },
    'mty:web/dom': dom,
    // v0.5 caps stubs — return Forbidden-like sentinels by default
    // so a sandboxed wasm module doesn't crash.
    'mty:caps/fs': {
      read: (_p, _pl) => 0,
      write: (_p, _pl, _d, _dl) => 0,
    },
    'mty:caps/net': { get: (_p, _pl) => 0 },
    'mty:caps/clock': { 'now-millis': () => BigInt(Date.now()) },
    'mty:caps/model': { invoke: (_p, _pl) => 0 },
  };

  const module = await WebAssembly.compile(wasmBytes);
  const instance = await WebAssembly.instantiate(module, imports);
  memory = instance.exports.memory;
  return { exports: instance.exports, memory };
}
