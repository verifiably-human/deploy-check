// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
//
// Writes the initial ContractState a deploy of a compactc build would commit:
// the constructor's ledger data plus one operation per circuit, serialized
// with the Compact runtime the build was compiled against. deploy-check's
// exact mode adds the verifier keys and costs the result with the ledger.
//
// Usage: node initial-state.mjs <build-dir> <out-file> [ctor-args-json]
//
// The Compact runtime is resolved from the build directory, i.e. from the
// node_modules of the project that compiled it, so its version always
// matches the build. Constructor arguments are a JSON array: numbers become
// BigInt (Field/Uint), "0x..." strings become Uint8Array (Bytes<n>), other
// values pass through. Constructors that call witnesses are not supported;
// serialize `initialState(...).currentContractState` yourself and pass it
// with --initial-state.

import { createRequire } from 'node:module';
import { copyFileSync, rmSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [buildArg, outFile, argsJson = '[]'] = process.argv.slice(2);
if (!buildArg || !outFile) {
  console.error('usage: node initial-state.mjs <build-dir> <out-file> [ctor-args-json]');
  process.exit(2);
}

const build = resolve(buildArg);
const entry = `${build}/contract/index.js`;
const require = createRequire(entry);

let runtimePath;
try {
  runtimePath = require.resolve('@midnight-ntwrk/compact-runtime');
} catch {
  console.error(
    `cannot resolve @midnight-ntwrk/compact-runtime from ${build}. ` +
      'Run deploy-check on a build inside a project that has it installed, or pass --initial-state.'
  );
  process.exit(3);
}

const runtime = await import(pathToFileURL(runtimePath).href);
// compactc emits ES module syntax in contract/index.js. If the project's
// package.json doesn't declare "type": "module", Node loads it as CommonJS
// and fails; fall back to a temporary .mjs copy beside it, so the Compact
// runtime still resolves from the project.
let contract;
try {
  contract = await import(pathToFileURL(entry).href);
} catch (err) {
  if (!(err instanceof SyntaxError)) throw err;
  const copy = `${build}/contract/.deploy-check-index.mjs`;
  copyFileSync(entry, copy);
  try {
    contract = await import(pathToFileURL(copy).href);
  } finally {
    rmSync(copy, { force: true });
  }
}

const convert = (v) =>
  typeof v === 'number'
    ? BigInt(v)
    : typeof v === 'string' && /^0x([0-9a-fA-F]{2})*$/.test(v)
      ? Uint8Array.from(Buffer.from(v.slice(2), 'hex'))
      : Array.isArray(v)
        ? v.map(convert)
        : v;
const args = JSON.parse(argsJson).map(convert);

const witnesses = new Proxy(
  {},
  {
    get(_target, name) {
      if (typeof name !== 'string' || name === 'then') return undefined;
      return () => {
        throw new Error(`constructor called witness \`${name}\`; pass --initial-state instead`);
      };
    },
  }
);

const init = await new contract.Contract(witnesses).initialState(
  runtime.createConstructorContext({}, '0'.repeat(64)),
  ...args
);
writeFileSync(outFile, init.currentContractState.serialize());
