// Starts a Web Worker that trunk built with wasm-bindgen's `no-modules`
// target, named by the query: `./worker_loader.js?varde-io-worker`.
//
// Trunk's own loader shim leaves the promise of `wasm_bindgen` unhandled,
// so a `.wasm` that fails to load only fires `unhandledrejection` in the
// worker, which the page never hears of. Rethrown from a task of its own,
// the failure is uncaught and reaches the page's `Worker.onerror`, as a
// script that failed to load does.
const name = self.location.search.slice(1);
importScripts(`./${name}.js`);
wasm_bindgen({ module_or_path: `./${name}_bg.wasm` }).catch((error) => {
  setTimeout(() => {
    throw error;
  });
});
