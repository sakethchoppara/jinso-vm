// Runs lumen.wasm off the main thread, so the page can kill a program that never ends.
// The wasm module reports back through emit(kind, ptr, len); each call becomes one message.
const KINDS = ['output', 'error', 'bytecode', 'ast', 'gc']; // same order as src/wasm.rs
const enc = new TextEncoder();
const dec = new TextDecoder();
let memory;

// no-cache revalidates with the server (a cheap 304 when unchanged) instead of trusting a stale copy.
const ready = WebAssembly.instantiateStreaming(fetch('lumen.wasm', { cache: 'no-cache' }), {
  env: {
    emit: (kind, ptr, len) => postMessage({ [KINDS[kind]]: dec.decode(new Uint8Array(memory.buffer, ptr, len)) }),
  },
}).then(r => {
  memory = r.instance.exports.memory;
  return r.instance.exports;
});

onmessage = async ({ data: { src, stress } }) => {
  let wasm;
  try {
    wasm = await ready;
  } catch (e) {
    postMessage({ crash: `couldn't load lumen.wasm (${e.message}). Build it with scripts/build-web.sh.` });
    return;
  }
  const bytes = enc.encode(src);
  const ptr = wasm.alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  try {
    const t0 = performance.now();
    wasm.run(ptr, bytes.length, stress ? 1 : 0);
    postMessage({ done: true, ms: performance.now() - t0 });
  } catch (e) {
    // A Rust panic traps the instance; the page replaces this worker.
    postMessage({ crash: `internal VM error: ${e.message}` });
  }
};
