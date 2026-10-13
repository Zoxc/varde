// Runs a wasm32-wasip1 program under Node's WASI, as Cargo's runner for
// that target (tools/ci.sh's `wasm` step runs the decision layer's
// same-bits tests so):
//
//   CARGO_TARGET_WASM32_WASIP1_RUNNER="node $PWD/tools/wasi_run.mjs" \
//       cargo test -p varde-decide --target wasm32-wasip1 -- same_bits
//
// The program sees its arguments and the environment, and no files.
import { readFile } from "node:fs/promises";
import { argv, env, exit } from "node:process";
import { WASI } from "node:wasi";

const [program, ...args] = argv.slice(2);
const wasi = new WASI({ version: "preview1", args: [program, ...args], env, returnOnExit: true });
const module = await WebAssembly.compile(await readFile(program));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
