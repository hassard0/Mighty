/// <reference types="vite/client" />

// v0.33 T3 — Vite ?worker / ?url / ?raw module declarations.
//
// Vite rewrites these suffixes at bundle time. TS needs ambient
// declarations to accept them as importable modules.

declare module "*?worker" {
  const workerConstructor: {
    new (): Worker;
  };
  export default workerConstructor;
}

declare module "*?worker&inline" {
  const workerConstructor: {
    new (): Worker;
  };
  export default workerConstructor;
}

declare module "*?url" {
  const url: string;
  export default url;
}

declare module "*?raw" {
  const src: string;
  export default src;
}

interface ImportMetaEnv {
  readonly USE_WASM_BACKEND?: boolean;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
