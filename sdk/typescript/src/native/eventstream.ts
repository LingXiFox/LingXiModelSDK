import type { SseEvent } from "./sse.ts";

export const amazonEventStream = "application/vnd.amazon.eventstream";
const utf8 = new TextDecoder("utf-8", { fatal: true });

function crc32(data: Uint8Array): number {
  let crc = -1;
  for (const byte of data) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (~crc) >>> 0;
}

function parseError(message: string): Error {
  return new Error(`Amazon Event Stream: ${message}`);
}

function decode(frame: Uint8Array): SseEvent {
  const view = new DataView(frame.buffer);
  const end = frame.length - 4;
  if (crc32(frame.subarray(0, end)) !== view.getUint32(end)) {
    throw parseError("invalid message checksum");
  }
  const payloadStart = 12 + view.getUint32(4);
  const headers = new Map<string, string>();
  let offset = 12;
  while (offset < payloadStart) {
    const keySize = frame[offset++];
    if (offset + keySize >= payloadStart) throw parseError("truncated header");
    const key = utf8.decode(frame.subarray(offset, offset + keySize));
    offset += keySize;
    const tag = frame[offset++];
    let size = [0, 0, 1, 2, 4, 8, -1, -1, 8, 16][tag];
    if (tag === 6 || tag === 7) {
      if (offset + 2 > payloadStart) throw parseError("truncated header size");
      size = view.getUint16(offset);
      offset += 2;
    }
    if (size === undefined || offset + size > payloadStart) {
      throw parseError("invalid header value");
    }
    if (key === ":message-type" || key === ":event-type") {
      if (tag !== 7 || headers.has(key)) throw parseError("invalid event header");
      headers.set(key, utf8.decode(frame.subarray(offset, offset + size)));
    }
    offset += size;
  }
  const kind = headers.get(":message-type");
  if (kind === "error" || kind === "exception") throw parseError("upstream sent an error frame");
  const name = headers.get(":event-type");
  if (kind !== "event" || !name) throw parseError("missing or invalid event header");
  const payload = utf8.decode(frame.subarray(payloadStart, end));
  JSON.parse(payload);
  return { event: name, data: `{${JSON.stringify(name)}:${payload}}`, id: null };
}

export async function* parseAmazonEventStream(stream: ReadableStream<Uint8Array>): AsyncGenerator<SseEvent> {
  const prelude = new Uint8Array(12);
  const view = new DataView(prelude.buffer);
  const reader = stream.getReader();
  let frame = prelude;
  let filled = 0;
  let received = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      received += value.length;
      if (received > 32 * 1024 * 1024) throw parseError("stream exceeded 32 MiB");
      let offset = 0;
      while (offset < value.length) {
        const take = Math.min(frame.length - filled, value.length - offset);
        frame.set(value.subarray(offset, offset + take), filled);
        filled += take;
        offset += take;
        if (filled !== frame.length) continue;
        if (frame === prelude) {
          if (crc32(prelude.subarray(0, 8)) !== view.getUint32(8)) {
            throw parseError("invalid prelude checksum");
          }
          const total = view.getUint32(0);
          const headers = view.getUint32(4);
          if (headers > 128 * 1024 || total < 16 + headers || total - 16 - headers > 16 * 1024 * 1024) {
            throw parseError("invalid frame size");
          }
          frame = new Uint8Array(total);
          frame.set(prelude);
        } else {
          yield decode(frame);
          frame = prelude;
          filled = 0;
        }
      }
    }
    if (filled !== 0) throw parseError("truncated frame");
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
