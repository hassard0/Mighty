// v0.5 dogfood Gap-2 — JS implementation of the `mty:web/dom`
// component interface declared in
// `crates/sdust-codegen-wasm/src/wit.rs`.
//
// The Stardust compiler emits core-wasm imports under the canonical
// `mty:web/dom` module with `(ptr, len)` argument pairs for each
// string. This shim wraps a WebAssembly Instance to satisfy those
// imports against `document.*`.
//
// Usage from the demo's index.html:
//
//   import { instantiateWithDomShim } from './dom-shim.js';
//   const inst = await instantiateWithDomShim(wasmBytes, {
//     log: (msg) => console.log(msg),
//   });
//   inst.exports.bump();
//
// The shim copies (ptr, len) pairs out of the module's linear memory
// using a TextDecoder, and copies return strings back in via a
// caller-allocated bump pointer at offset 8192 (kept above the data
// section's initial reservation).

const RETURN_BUF_OFFSET = 8192;
const RETURN_BUF_CAPACITY = 4096;

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
    // [len:i32 | bytes...] starting at RETURN_BUF_OFFSET
    view.setUint32(RETURN_BUF_OFFSET, bytes.length, true);
    new Uint8Array(mem.buffer, RETURN_BUF_OFFSET + 4, bytes.length).set(bytes);
    return RETURN_BUF_OFFSET;
  }

  return {
    'set-text': (idPtr, idLen, textPtr, textLen) => {
      const id = readStr(idPtr, idLen);
      const text = readStr(textPtr, textLen);
      const el = document.getElementById(id);
      if (el) el.textContent = text;
    },
    'get-text': (idPtr, idLen) => {
      const id = readStr(idPtr, idLen);
      const el = document.getElementById(id);
      const s = el ? (el.textContent ?? '') : '';
      return writeReturnStr(s);
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
    'query': (selPtr, selLen) => {
      const sel = readStr(selPtr, selLen);
      const el = document.querySelector(sel);
      // Option<string>: returns 0 for none, or a pointer to len-prefixed bytes.
      if (!el) return 0;
      return writeReturnStr(el.id || '');
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
