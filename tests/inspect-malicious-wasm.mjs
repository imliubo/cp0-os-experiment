import fs from "node:fs";

if (process.argv.length !== 4) {
  throw new Error("usage: inspect-malicious-wasm.mjs memory-hog.wasm ambient-authority.wasm");
}

const memoryHog = new WebAssembly.Module(fs.readFileSync(process.argv[2]));
const ambientAuthority = new WebAssembly.Module(fs.readFileSync(process.argv[3]));
const memoryImports = WebAssembly.Module.imports(memoryHog);
const ambientImports = WebAssembly.Module.imports(ambientAuthority);

if (memoryImports.length !== 0) {
  throw new Error(`memory hog unexpectedly imports ambient APIs: ${JSON.stringify(memoryImports)}`);
}

const importedNames = ambientImports.map(({ module, name }) => `${module}.${name}`).sort();
const expected = [
  "wasi_snapshot_preview1.path_open",
  "wasi_snapshot_preview1.sock_open",
];
if (JSON.stringify(importedNames) !== JSON.stringify(expected)) {
  throw new Error(`unexpected ambient imports: ${JSON.stringify(importedNames)}`);
}

console.log("malicious WASM import policy fixtures passed");
