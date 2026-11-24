export interface SseEvent {
  readonly event: string | null;
  readonly data: string;
  readonly id: string | null;
}

export async function* parseSseStream(
  stream: ReadableStream<Uint8Array>
): AsyncGenerator<SseEvent> {
  const reader = stream.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";
  let eventName: string | null = null;
  let dataLines: string[] = [];
  let lastID: string | null = null;

  const flush = (): SseEvent | null => {
    if (dataLines.length === 0 && eventName === null) return null;
    const ev: SseEvent = {
      event: eventName,
      data: dataLines.join("\n"),
      id: lastID,
    };
    eventName = null;
    dataLines = [];
    return ev;
  };

  const processLine = (line: string) => {
    if (line.startsWith(":")) return;
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    let val = colon === -1 ? "" : line.slice(colon + 1);
    if (val.startsWith(" ")) val = val.slice(1);
    if (field === "event") eventName = val;
    else if (field === "data") dataLines.push(val);
    else if (field === "id") lastID = val;
  };

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      while (true) {
        const nl = buffer.indexOf("\n");
        if (nl === -1) break;
        let line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (line.endsWith("\r")) line = line.slice(0, -1);
        if (line === "") {
          const ev = flush();
          if (ev) yield ev;
        } else {
          processLine(line);
        }
      }
    }
    buffer += decoder.decode();
    if (buffer.length > 0) {
      const line = buffer.endsWith("\r") ? buffer.slice(0, -1) : buffer;
      if (line) processLine(line);
    }
    const last = flush();
    if (last) yield last;
  } finally {
    reader.releaseLock();
  }
}

export async function* parseNdjsonStream(
  stream: ReadableStream<Uint8Array>
): AsyncGenerator<SseEvent> {
  const reader = stream.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      while (true) {
        const nl = buffer.indexOf("\n");
        if (nl === -1) break;
        const line = buffer.slice(0, nl).trim();
        buffer = buffer.slice(nl + 1);
        if (line) yield { event: null, data: line, id: null };
      }
    }
    buffer += decoder.decode();
    const tail = buffer.trim();
    if (tail) yield { event: null, data: tail, id: null };
  } finally {
    reader.releaseLock();
  }
}
