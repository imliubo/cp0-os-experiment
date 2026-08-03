#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";

const ERROR_DENIED = -1;
const ERROR_UNAVAILABLE = -2;
const ERROR_INVALID = -3;
const ERROR_LIMIT = -4;
const CAMERA_WIDTH = 320;
const CAMERA_HEIGHT = 170;

function runController() {
  const options = parseArguments(process.argv.slice(2));
  const manifest = JSON.parse(fs.readFileSync(options.manifest, "utf8"));
  const wasm = fs.readFileSync(options.wasm);
  const height = manifest.display === "immersive" ? 170 : 150;
  const started = Date.now();
  let lastFrame = null;
  let lastMetrics = emptyMetrics();
  let finished = false;
  const notifications = [];

  const worker = new Worker(new URL(import.meta.url), {
    workerData: {
      wasm,
      manifest,
      height,
      permissionMode: options.permissions,
      keys: parseKeys(options.keys),
      mediaActions: parseMediaActions(options.mediaActions),
    },
  });

  worker.on("message", (message) => {
    if (message.type === "frame") {
      lastFrame = new Uint8Array(message.frame);
      lastMetrics = message.metrics;
    } else if (message.type === "notification") {
      notifications.push({ title: message.title, body: message.body });
    } else if (message.type === "exit") {
      lastMetrics = message.metrics;
      finish(message.code === 0 ? null : `application returned ${message.code}`);
    } else if (message.type === "error") {
      finish(message.message);
    }
  });
  worker.on("error", (error) => finish(error.stack || String(error)));

  const timer = setTimeout(async () => {
    await worker.terminate();
    finish(null);
  }, options.duration);

  function finish(error) {
    if (finished) return;
    finished = true;
    clearTimeout(timer);
    if (error) {
      process.stderr.write(`cp0-simulator: ${error}\n`);
      process.exitCode = 1;
      return;
    }
    if (!lastFrame || lastFrame.length !== 320 * height * 2) {
      process.stderr.write("cp0-simulator: application did not present a valid frame\n");
      process.exitCode = 1;
      return;
    }
    fs.mkdirSync(path.dirname(options.output), { recursive: true });
    fs.writeFileSync(options.output, rgb565ToPpm(lastFrame, 320, height));
    const profile = {
      schema_version: 1,
      app_id: manifest.id,
      app_version: manifest.version,
      duration_ms: Date.now() - started,
      configured_duration_ms: options.duration,
      permission_mode: options.permissions,
      scripted_keys: options.keys ? options.keys.split(",").filter(Boolean) : [],
      scripted_media_actions: options.mediaActions
        ? options.mediaActions.split(",").filter(Boolean)
        : [],
      wasm_bytes: wasm.length,
      frame_bytes: lastFrame.length,
      notifications,
      ...lastMetrics,
    };
    fs.mkdirSync(path.dirname(options.profile), { recursive: true });
    fs.writeFileSync(options.profile, `${JSON.stringify(profile, null, 2)}\n`);
    process.stdout.write(`simulated ${manifest.id} ${manifest.version}\n`);
    process.stdout.write(`frame: ${options.output}\nprofile: ${options.profile}\n`);
  }
}

async function runApplication() {
  const { wasm, manifest, height, permissionMode, keys, mediaActions } = workerData;
  const metrics = emptyMetrics();
  const permissions = new Set((manifest.permissions || []).map((request) => request.name));
  const keyQueue = [...keys];
  const mediaActionQueue = [...mediaActions];
  let mediaState = 0;
  let mediaSupportedActions = 0;
  const storage = new Map();
  const storageQuota = Number(manifest.resources.storage_mb) * 1024 * 1024;
  let storageBytes = 0;
  const gpio = [false, false, false, false];
  const document = new TextEncoder().encode("CardputerZero simulator document\n");
  const sleeper = new Int32Array(new SharedArrayBuffer(4));
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let instance = null;
  let started = Date.now();

  function memory() {
    const value = instance?.exports?.memory;
    if (!(value instanceof WebAssembly.Memory)) throw new Error("WASM memory export is missing");
    return value;
  }

  function range(pointer, length) {
    const start = pointer >>> 0;
    const size = length >>> 0;
    const end = start + size;
    if (end < start || end > memory().buffer.byteLength) throw new RangeError("linear memory range");
    return new Uint8Array(memory().buffer, start, size);
  }

  function text(pointer, length) {
    return decoder.decode(range(pointer, length));
  }

  function allowed(capability) {
    metrics.capability_calls[capability] = (metrics.capability_calls[capability] || 0) + 1;
    return permissions.has(capability) && permissionMode === "allow";
  }

  function status(capability) {
    return allowed(capability) ? 0 : ERROR_DENIED;
  }

  function snapshot(frame = null) {
    metrics.memory_pages = memory().buffer.byteLength / 65536;
    metrics.storage_bytes = storageBytes;
    metrics.storage_keys = storage.size;
    if (frame) {
      parentPort.postMessage({ type: "frame", frame, metrics: structuredClone(metrics) }, [frame]);
    }
  }

  const host = {
    cp0_monotonic_milliseconds() {
      metrics.host_calls += 1;
      return BigInt(Date.now() - started);
    },
    cp0_wait_event(timeout) {
      metrics.host_calls += 1;
      Atomics.wait(sleeper, 0, 0, Math.max(0, Math.min(timeout, 20)));
      return 0;
    },
    cp0_display_dimensions() {
      metrics.host_calls += 1;
      return 320 | (height << 16);
    },
    cp0_present_rgb565(pointer, pixelBytes, damagePointer, damageBytes) {
      metrics.host_calls += 1;
      if ((pixelBytes >>> 0) !== 320 * height * 2 || (damageBytes >>> 0) > 32 * 8) {
        return ERROR_INVALID;
      }
      range(damagePointer, damageBytes);
      const copy = range(pointer, pixelBytes).slice().buffer;
      metrics.frames_presented += 1;
      snapshot(copy);
      return 0;
    },
    cp0_poll_key_event(pointer, eventBytes, timeout) {
      metrics.host_calls += 1;
      if ((eventBytes >>> 0) !== 8 || timeout < 0 || timeout > 1000) return ERROR_INVALID;
      const event = keyQueue.shift();
      if (!event) {
        Atomics.wait(sleeper, 0, 0, Math.max(0, Math.min(timeout, 20)));
        return 0;
      }
      const output = range(pointer, eventBytes);
      output.fill(0);
      output[0] = event.code & 0xff;
      output[1] = (event.code >>> 8) & 0xff;
      output[2] = 1;
      output[4] = event.modifiers;
      metrics.key_events += 1;
      return 1;
    },
    cp0_post_notification(titlePointer, titleLength, bodyPointer, bodyLength) {
      metrics.host_calls += 1;
      if (!allowed("notifications.post")) return ERROR_DENIED;
      parentPort.postMessage({
        type: "notification",
        title: text(titlePointer, titleLength),
        body: text(bodyPointer, bodyLength),
      });
      return 0;
    },
    cp0_http_get(urlPointer, urlLength, bodyPointer, bodyCapacity) {
      metrics.host_calls += 1;
      if (!allowed("network.client")) return BigInt(ERROR_DENIED);
      const url = text(urlPointer, urlLength);
      if (!url.startsWith("https://")) return BigInt(ERROR_INVALID);
      const body = new TextEncoder().encode("simulated HTTPS response");
      if (body.length > (bodyCapacity >>> 0)) return BigInt(ERROR_LIMIT);
      range(bodyPointer, bodyCapacity).set(body);
      return (200n << 32n) | BigInt(body.length);
    },
    cp0_document_open() {
      metrics.host_calls += 1;
      if (!allowed("documents.open")) return BigInt(ERROR_DENIED);
      return (1n << 32n) | BigInt(document.length);
    },
    cp0_document_read(handle, offset, pointer, capacity) {
      metrics.host_calls += 1;
      if (handle !== 1) return BigInt(ERROR_INVALID);
      const start = Number(offset);
      const count = Math.max(0, Math.min(capacity >>> 0, document.length - start));
      range(pointer, capacity).set(document.subarray(start, start + count));
      return BigInt(count);
    },
    cp0_document_close(handle) {
      metrics.host_calls += 1;
      return handle === 1 ? 0 : ERROR_INVALID;
    },
    cp0_audio_play_pcm_s16le(pointer, bytes) {
      metrics.host_calls += 1;
      range(pointer, bytes);
      return status("audio.playback");
    },
    cp0_audio_capture_pcm_s16le(pointer, bytes) {
      metrics.host_calls += 1;
      if (!allowed("audio.capture")) return ERROR_DENIED;
      range(pointer, bytes).fill(0);
      return bytes | 0;
    },
    cp0_camera_capture_rgb565(pointer, bytes) {
      metrics.host_calls += 1;
      if (!allowed("camera.capture")) return ERROR_DENIED;
      if ((bytes >>> 0) !== CAMERA_WIDTH * CAMERA_HEIGHT * 2) return ERROR_INVALID;
      const output = range(pointer, bytes);
      for (let y = 0; y < CAMERA_HEIGHT; y += 1) {
        for (let x = 0; x < CAMERA_WIDTH; x += 1) {
          const pixel = (((x >>> 3) & 0x1f) << 11) | (((y >>> 2) & 0x3f) << 5) | 0x0f;
          const offset = (y * CAMERA_WIDTH + x) * 2;
          output[offset] = pixel & 0xff;
          output[offset + 1] = pixel >>> 8;
        }
      }
      return 0;
    },
    cp0_gpio_read(line) {
      metrics.host_calls += 1;
      if (!allowed("hardware.gpio")) return ERROR_DENIED;
      return line >= 0 && line < gpio.length ? Number(gpio[line]) : ERROR_INVALID;
    },
    cp0_gpio_write(line, value) {
      metrics.host_calls += 1;
      if (!allowed("hardware.gpio")) return ERROR_DENIED;
      if (line < 0 || line >= gpio.length || (value !== 0 && value !== 1)) return ERROR_INVALID;
      gpio[line] = value === 1;
      return 0;
    },
    cp0_lora_send(pointer, length) {
      metrics.host_calls += 1;
      range(pointer, length);
      return status("radio.lora");
    },
    cp0_lora_receive(pointer, capacity, metadataPointer, metadataBytes, timeout) {
      metrics.host_calls += 1;
      range(pointer, capacity);
      range(metadataPointer, metadataBytes);
      if (timeout < 0 || timeout > 1000) return ERROR_INVALID;
      return allowed("radio.lora") ? 0 : ERROR_DENIED;
    },
    cp0_storage_put(keyPointer, keyLength, valuePointer, valueLength) {
      metrics.host_calls += 1;
      const key = text(keyPointer, keyLength);
      const value = range(valuePointer, valueLength).slice();
      const existing = storage.get(key);
      if (!existing && storage.size >= 256) return ERROR_LIMIT;
      const projected = storageBytes - (existing?.length || 0) + value.length;
      if (projected > storageQuota) return ERROR_LIMIT;
      storage.set(key, value);
      storageBytes = projected;
      return 0;
    },
    cp0_storage_get(keyPointer, keyLength, valuePointer, valueCapacity) {
      metrics.host_calls += 1;
      const value = storage.get(text(keyPointer, keyLength));
      if (!value) return 0;
      if (value.length > (valueCapacity >>> 0)) return ERROR_LIMIT;
      range(valuePointer, valueCapacity).set(value);
      return value.length;
    },
    cp0_storage_delete(keyPointer, keyLength) {
      metrics.host_calls += 1;
      const key = text(keyPointer, keyLength);
      const value = storage.get(key);
      if (!value) return 0;
      storage.delete(key);
      storageBytes -= value.length;
      return 1;
    },
    cp0_intent_send(actionPointer, actionLength, payloadPointer, payloadLength) {
      metrics.host_calls += 1;
      text(actionPointer, actionLength);
      range(payloadPointer, payloadLength);
      return 0;
    },
    cp0_intent_take(actionPointer, actionCapacity, payloadPointer, payloadCapacity) {
      metrics.host_calls += 1;
      range(actionPointer, actionCapacity);
      range(payloadPointer, payloadCapacity);
      return 0n;
    },
    cp0_media_session_update(state, supportedActions) {
      metrics.host_calls += 1;
      const normalizedState = state >>> 0;
      const normalizedActions = supportedActions >>> 0;
      if (
        normalizedState > 2 ||
        (normalizedActions & ~0x07) !== 0 ||
        ((normalizedState === 0) !== (normalizedActions === 0))
      ) {
        return ERROR_INVALID;
      }
      mediaState = normalizedState;
      mediaSupportedActions = normalizedActions;
      metrics.media_session_updates += 1;
      return 0;
    },
    cp0_media_take_action() {
      metrics.host_calls += 1;
      if (mediaState === 0) return 0;
      while (mediaActionQueue.length > 0) {
        const action = mediaActionQueue.shift();
        const bit = 1 << (action - 1);
        if ((mediaSupportedActions & bit) !== 0) {
          metrics.media_actions_taken += 1;
          return action;
        }
      }
      return 0;
    },
  };

  const result = await WebAssembly.instantiate(wasm, { cardputerzero: host });
  instance = result.instance;
  started = Date.now();
  metrics.memory_pages = memory().buffer.byteLength / 65536;
  const code = instance.exports.main();
  snapshot();
  parentPort.postMessage({ type: "exit", code, metrics });
}

function emptyMetrics() {
  return {
    frames_presented: 0,
    key_events: 0,
    host_calls: 0,
    memory_pages: 0,
    storage_bytes: 0,
    storage_keys: 0,
    media_session_updates: 0,
    media_actions_taken: 0,
    capability_calls: {},
  };
}

function parseArguments(arguments_) {
  const options = { duration: 1000, permissions: "deny", keys: "", mediaActions: "" };
  for (let index = 0; index < arguments_.length; index += 1) {
    const name = arguments_[index];
    const value = arguments_[++index];
    if (value === undefined) throw new Error(`missing value for ${name}`);
    if (name === "--wasm") options.wasm = value;
    else if (name === "--manifest") options.manifest = value;
    else if (name === "--duration") options.duration = Number(value);
    else if (name === "--permissions") options.permissions = value;
    else if (name === "--keys") options.keys = value;
    else if (name === "--media-actions") options.mediaActions = value;
    else if (name === "--output") options.output = value;
    else if (name === "--profile") options.profile = value;
    else throw new Error(`unknown option ${name}`);
  }
  if (!options.wasm || !options.manifest || !options.output || !options.profile) {
    throw new Error("--wasm, --manifest, --output and --profile are required");
  }
  if (!Number.isInteger(options.duration) || options.duration < 100 || options.duration > 30000) {
    throw new Error("duration must be between 100 and 30000 milliseconds");
  }
  if (!new Set(["allow", "deny"]).has(options.permissions)) {
    throw new Error("permissions must be allow or deny");
  }
  return options;
}

function parseMediaActions(value) {
  if (!value) return [];
  const actions = { "play-pause": 1, previous: 2, next: 3 };
  return value.split(",").map((name) => {
    const action = actions[name];
    if (!action) throw new Error(`unknown media action ${name}`);
    return action;
  });
}

function parseKeys(value) {
  if (!value) return [];
  return value.split(",").filter(Boolean).map((name) => {
    const normalized = name.toLowerCase();
    const code = KEY_CODES[normalized];
    if (!code) throw new Error(`unknown key name ${name}`);
    return { code, modifiers: 0 };
  });
}

function rgb565ToPpm(frame, width, height) {
  const header = Buffer.from(`P6\n${width} ${height}\n255\n`, "ascii");
  const rgb = Buffer.alloc(width * height * 3);
  for (let index = 0; index < width * height; index += 1) {
    const pixel = frame[index * 2] | (frame[index * 2 + 1] << 8);
    rgb[index * 3] = ((pixel >>> 11) & 0x1f) * 255 / 31;
    rgb[index * 3 + 1] = ((pixel >>> 5) & 0x3f) * 255 / 63;
    rgb[index * 3 + 2] = (pixel & 0x1f) * 255 / 31;
  }
  return Buffer.concat([header, rgb]);
}

const KEY_CODES = {
  esc: 1,
  backspace: 14,
  enter: 28,
  space: 57,
  left: 105,
  right: 106,
  up: 103,
  down: 108,
  f1: 59,
  f2: 60,
  f3: 61,
  f4: 62,
  "0": 11,
  "1": 2,
  "2": 3,
  "3": 4,
  "4": 5,
  "5": 6,
  "6": 7,
  "7": 8,
  "8": 9,
  "9": 10,
  plus: 78,
  minus: 74,
  multiply: 55,
  divide: 98,
  equal: 28,
  a: 30,
  b: 48,
  c: 46,
  d: 32,
  e: 18,
  f: 33,
  g: 34,
  h: 35,
  i: 23,
  j: 36,
  k: 37,
  l: 38,
  m: 50,
  n: 49,
  o: 24,
  p: 25,
  q: 16,
  r: 19,
  s: 31,
  t: 20,
  u: 22,
  v: 47,
  w: 17,
  x: 45,
  y: 21,
  z: 44,
};

if (isMainThread) {
  runController();
} else {
  runApplication().catch((error) => {
    parentPort.postMessage({ type: "error", message: error.stack || String(error) });
  });
}
