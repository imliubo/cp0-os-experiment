#!/usr/bin/env node

import { once } from "node:events";
import { constants as fsConstants } from "node:fs";
import {
  chmod,
  lstat,
  mkdir,
  open,
  realpath,
  rename,
  stat,
  writeFile,
} from "node:fs/promises";
import http from "node:http";
import path from "node:path";

const CONTROL_SCHEMA_VERSION = 1;
const MAX_CONTROL_BYTES = 4096;
const MIN_THROTTLE_BYTES_PER_SECOND = 4096;
const MAX_THROTTLE_BYTES_PER_SECOND = 4 * 1024 * 1024;
const DEFAULT_THROTTLE_BYTES_PER_SECOND = 512 * 1024;

function usage() {
  throw new Error(
    "usage: test-store-origin.mjs serve STORE_ROOT CONTROL_FILE [PORT] | " +
      "set CONTROL_FILE <v1|v1-slow|v2|offline-v2> [BYTES_PER_SECOND] | " +
      "status CONTROL_FILE",
  );
}

function exactKeys(value, expected) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  return actual.length === wanted.length && actual.every((key, index) => key === wanted[index]);
}

function validateControl(value) {
  if (
    !exactKeys(value, [
      "schema_version",
      "release",
      "online",
      "package_bytes_per_second",
    ]) ||
    value.schema_version !== CONTROL_SCHEMA_VERSION ||
    !["v1", "v2"].includes(value.release) ||
    typeof value.online !== "boolean" ||
    !Number.isSafeInteger(value.package_bytes_per_second) ||
    value.package_bytes_per_second < 0 ||
    value.package_bytes_per_second > MAX_THROTTLE_BYTES_PER_SECOND ||
    (!value.online && value.package_bytes_per_second !== 0) ||
    (value.release === "v2" && value.package_bytes_per_second !== 0) ||
    (value.package_bytes_per_second > 0 &&
      value.package_bytes_per_second < MIN_THROTTLE_BYTES_PER_SECOND)
  ) {
    throw new Error("Store origin control file is invalid");
  }
  return value;
}

async function readControl(controlPath) {
  const handle = await open(
    controlPath,
    fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0),
  );
  try {
    const metadata = await handle.stat();
    if (!metadata.isFile() || metadata.size < 2 || metadata.size > MAX_CONTROL_BYTES) {
      throw new Error("Store origin control file has an invalid type or size");
    }
    return validateControl(JSON.parse(await handle.readFile("utf8")));
  } finally {
    await handle.close();
  }
}

async function ensureNotSymlink(filePath) {
  try {
    if ((await lstat(filePath)).isSymbolicLink()) {
      throw new Error(`refusing symbolic output path: ${filePath}`);
    }
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
}

async function writeJsonAtomic(filePath, value, mode) {
  const absolute = path.resolve(filePath);
  await mkdir(path.dirname(absolute), { recursive: true, mode: 0o700 });
  await ensureNotSymlink(absolute);
  const temporary = `${absolute}.tmp-${process.pid}-${Date.now()}`;
  await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
    mode,
  });
  await rename(temporary, absolute);
  await chmod(absolute, mode);
}

function controlForMode(mode, rateText) {
  let rate = 0;
  if (mode === "v1-slow") {
    rate = rateText === undefined ? DEFAULT_THROTTLE_BYTES_PER_SECOND : Number(rateText);
  } else if (rateText !== undefined) {
    usage();
  }
  const values = {
    v1: { release: "v1", online: true },
    "v1-slow": { release: "v1", online: true },
    v2: { release: "v2", online: true },
    "offline-v2": { release: "v2", online: false },
  };
  const selected = values[mode];
  if (selected === undefined) {
    usage();
  }
  return validateControl({
    schema_version: CONTROL_SCHEMA_VERSION,
    release: selected.release,
    online: selected.online,
    package_bytes_per_second: rate,
  });
}

function sendError(response, status, message, extraHeaders = {}) {
  const body = `${message}\n`;
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Length": Buffer.byteLength(body),
    "Content-Type": "text/plain; charset=utf-8",
    "X-Content-Type-Options": "nosniff",
    ...extraHeaders,
  });
  response.end(body);
}

function assetForPath(control, pathname) {
  const version = control.release === "v1" ? "1.0.0" : "1.1.0";
  if (pathname === "/catalog.json") {
    return { relative: `catalog-${control.release}/catalog.json`, package: false };
  }
  if (pathname === "/store.pub") {
    return { relative: `catalog-${control.release}/store.pub`, package: false };
  }
  if (pathname === `/apps/dev.cardputerzero.store-test/${version}.capp`) {
    return {
      relative: `catalog-${control.release}/apps/dev.cardputerzero.store-test/${version}.capp`,
      package: true,
    };
  }
  return null;
}

function parseRange(header, size) {
  if (header === undefined) {
    return { offset: 0, partial: false };
  }
  const matched = /^bytes=(0|[1-9][0-9]*)-$/.exec(header);
  if (matched === null) {
    return null;
  }
  const offset = Number(matched[1]);
  if (!Number.isSafeInteger(offset) || offset >= size) {
    return null;
  }
  return { offset, partial: true };
}

function contentType(filePath) {
  return filePath.endsWith(".json")
    ? "application/json"
    : "application/octet-stream";
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitWritable(response) {
  if (response.destroyed) {
    return false;
  }
  await Promise.race([once(response, "drain"), once(response, "close")]);
  return !response.destroyed;
}

async function streamFile(handle, response, offset, length, rate) {
  const chunkSize =
    rate === 0 ? 64 * 1024 : Math.max(1024, Math.min(64 * 1024, Math.floor(rate / 10)));
  const buffer = Buffer.alloc(chunkSize);
  let position = offset;
  let remaining = length;
  while (remaining > 0 && !response.destroyed) {
    const wanted = Math.min(buffer.length, remaining);
    const { bytesRead } = await handle.read(buffer, 0, wanted, position);
    if (bytesRead === 0) {
      throw new Error("Store asset ended before its declared size");
    }
    if (!response.write(buffer.subarray(0, bytesRead)) && !(await waitWritable(response))) {
      return;
    }
    position += bytesRead;
    remaining -= bytesRead;
    if (rate > 0 && remaining > 0) {
      await delay(Math.ceil((bytesRead * 1000) / rate));
    }
  }
  if (!response.destroyed) {
    response.end();
  }
}

async function openAsset(root, relative) {
  const resolved = await realpath(path.join(root, relative));
  if (!resolved.startsWith(`${root}${path.sep}`)) {
    throw Object.assign(new Error("asset escaped Store root"), { code: "ENOENT" });
  }
  const handle = await open(resolved, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0));
  const metadata = await handle.stat();
  if (!metadata.isFile()) {
    await handle.close();
    throw Object.assign(new Error("asset is not a regular file"), { code: "ENOENT" });
  }
  return { handle, metadata, resolved };
}

async function handleRequest(root, controlPath, request, response) {
  let control;
  try {
    control = await readControl(controlPath);
  } catch (error) {
    console.error(`test-store-origin: ${error.message}`);
    sendError(response, 503, "origin control unavailable", { "Retry-After": "1" });
    return;
  }
  if (!control.online) {
    sendError(response, 503, "test origin offline", { "Retry-After": "1" });
    return;
  }
  if (!["GET", "HEAD"].includes(request.method)) {
    sendError(response, 405, "method not allowed", { Allow: "GET, HEAD" });
    return;
  }
  let parsed;
  try {
    parsed = new URL(request.url, "http://127.0.0.1");
  } catch {
    sendError(response, 400, "invalid request target");
    return;
  }
  if (parsed.search !== "") {
    sendError(response, 404, "not found");
    return;
  }
  const asset = assetForPath(control, parsed.pathname);
  if (asset === null) {
    sendError(response, 404, "not found");
    return;
  }
  let opened;
  try {
    opened = await openAsset(root, asset.relative);
  } catch (error) {
    if (error?.code === "ENOENT") {
      sendError(response, 404, "not found");
      return;
    }
    throw error;
  }
  const { handle, metadata, resolved } = opened;
  try {
    const range = asset.package ? parseRange(request.headers.range, metadata.size) : null;
    if ((asset.package && range === null) || (!asset.package && request.headers.range !== undefined)) {
      sendError(response, 416, "range not satisfiable", {
        "Content-Range": `bytes */${metadata.size}`,
      });
      return;
    }
    const offset = range?.offset ?? 0;
    const length = metadata.size - offset;
    const status = range?.partial ? 206 : 200;
    const headers = {
      "Cache-Control": "no-store",
      "Content-Length": length,
      "Content-Type": contentType(resolved),
      "X-Content-Type-Options": "nosniff",
    };
    if (asset.package) {
      headers["Accept-Ranges"] = "bytes";
    }
    if (range?.partial) {
      headers["Content-Range"] = `bytes ${offset}-${metadata.size - 1}/${metadata.size}`;
    }
    response.writeHead(status, headers);
    console.log(
      JSON.stringify({
        method: request.method,
        status,
        path: parsed.pathname,
        release: control.release,
        offset,
        bytes: length,
        package_bytes_per_second: asset.package ? control.package_bytes_per_second : 0,
      }),
    );
    if (request.method === "HEAD") {
      response.end();
      return;
    }
    await streamFile(
      handle,
      response,
      offset,
      length,
      asset.package ? control.package_bytes_per_second : 0,
    );
  } finally {
    await handle.close();
  }
}

async function serve(storeRoot, controlFile, portText = "18080") {
  const port = Number(portText);
  if (!Number.isSafeInteger(port) || port < 0 || port > 65535) {
    throw new Error("Store origin port must be between 0 and 65535");
  }
  const root = await realpath(storeRoot);
  if (!(await stat(root)).isDirectory()) {
    throw new Error("Store origin root is not a directory");
  }
  await readControl(path.resolve(controlFile));
  const server = http.createServer(
    {
      headersTimeout: 10_000,
      keepAliveTimeout: 5_000,
      maxHeaderSize: 8 * 1024,
      requestTimeout: 60_000,
    },
    (request, response) => {
      handleRequest(root, path.resolve(controlFile), request, response).catch((error) => {
        console.error(`test-store-origin: request failed: ${error.message}`);
        if (!response.headersSent) {
          sendError(response, 500, "origin failure");
        } else {
          response.destroy();
        }
      });
    },
  );
  server.maxRequestsPerSocket = 32;
  server.listen({ host: "127.0.0.1", port, exclusive: true });
  await once(server, "listening");
  const address = server.address();
  const ready = {
    schema_version: 1,
    host: "127.0.0.1",
    port: address.port,
    pid: process.pid,
  };
  if (process.env.CP0_TEST_STORE_READY_FILE !== undefined) {
    await writeJsonAtomic(process.env.CP0_TEST_STORE_READY_FILE, ready, 0o600);
  }
  console.log(`test Store origin: http://127.0.0.1:${address.port}`);
  const stop = () => server.close();
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
  await once(server, "close");
}

async function main() {
  const [command, ...arguments_] = process.argv.slice(2);
  switch (command) {
    case "serve":
      if (arguments_.length < 2 || arguments_.length > 3) usage();
      await serve(arguments_[0], arguments_[1], arguments_[2]);
      break;
    case "set": {
      if (arguments_.length < 2 || arguments_.length > 3) usage();
      const control = controlForMode(arguments_[1], arguments_[2]);
      await writeJsonAtomic(arguments_[0], control, 0o600);
      console.log(JSON.stringify(control, null, 2));
      break;
    }
    case "status":
      if (arguments_.length !== 1) usage();
      console.log(JSON.stringify(await readControl(path.resolve(arguments_[0])), null, 2));
      break;
    default:
      usage();
  }
}

main().catch((error) => {
  console.error(`test-store-origin: ${error.message}`);
  process.exitCode = 1;
});
