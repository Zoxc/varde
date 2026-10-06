// Starts a Web Worker, a module worker running an instance of the page's
// own wasm. The page posts it first the compiled `WebAssembly.Module`, the
// URL of wasm-bindgen's glue and the worker's role (see
// `varde_lane::page::Host::start`); the worker imports the glue,
// instantiates the module and serves the role, whose code then takes the
// messages from the page.
//
// Whatever fails here is rethrown from a task of its own, so it's
// uncaught and reaches the page's `Worker.onerror`, as a script that
// failed to load does, rather than only firing `unhandledrejection` in
// the worker, which the page never hears of.
self.onmessage = async (event) => {
  self.onmessage = null;
  try {
    const [module, glue, role] = event.data;
    const bindings = await import(glue);
    await bindings.default({ module_or_path: module });
    bindings.serve_worker(role);
  } catch (error) {
    setTimeout(() => {
      throw error;
    });
  }
};
