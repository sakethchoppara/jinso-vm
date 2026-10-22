// Runs lumen.wasm off the main thread, so the page can kill a program that never ends.
const ready = WebAssembly.instantiateStreaming(fetch('lumen.wasm')).then(r => r.instance.exports);
const enc = new TextEncoder();
const dec = new TextDecoder();

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
    const len = wasm.run(ptr, bytes.length, stress ? 1 : 0);
    const ms = performance.now() - t0;
    const text = dec.decode(new Uint8Array(wasm.memory.buffer, wasm.result_ptr(), len));
    const [output, error, bytecode, ast, gc] = text.split('\0');
    postMessage({ output, error, bytecode, ast, gc, ms });
  } catch (e) {
    // A Rust panic traps the instance; the page replaces this worker.
    postMessage({ crash: `internal VM error: ${e.message}` });
  }
};
