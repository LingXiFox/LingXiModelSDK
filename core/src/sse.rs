#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

/// Incremental line-oriented SSE parser handling arbitrary chunk boundaries.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    last_id: Option<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for bytes in chunk.split_inclusive(|byte| *byte == b'\n') {
            let Some(bytes) = bytes.strip_suffix(b"\n") else {
                self.buffer.extend_from_slice(bytes);
                break;
            };
            self.buffer.extend_from_slice(bytes);
            let line = String::from_utf8_lossy(&self.buffer);
            let line = line.strip_suffix('\r').unwrap_or(line.as_ref()).to_owned();
            self.buffer.clear();
            if line.is_empty() {
                if let Some(ev) = self.flush_event() {
                    events.push(ev);
                }
                continue;
            }
            self.process_line(&line);
        }
        events
    }

    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let bytes = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&bytes);
            let line = line.strip_suffix('\r').unwrap_or(line.as_ref());
            if !line.is_empty() {
                self.process_line(line);
            }
        }
        if let Some(ev) = self.flush_event() {
            events.push(ev);
        }
        events
    }

    fn process_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event_name = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            "id" => self.last_id = Some(value.to_string()),
            _ => {}
        }
    }

    fn flush_event(&mut self) -> Option<SseEvent> {
        if self.data_lines.is_empty() && self.event_name.is_none() {
            return None;
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        let event = self.event_name.take();
        Some(SseEvent {
            event,
            data,
            id: self.last_id.clone(),
        })
    }
}

/// Incremental NDJSON parser for Ollama and NDJSON relays.
#[derive(Debug, Default)]
pub struct NdjsonParser {
    buffer: Vec<u8>,
}

impl NdjsonParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for bytes in chunk.split_inclusive(|byte| *byte == b'\n') {
            let Some(bytes) = bytes.strip_suffix(b"\n") else {
                self.buffer.extend_from_slice(bytes);
                break;
            };
            self.buffer.extend_from_slice(bytes);
            let line = String::from_utf8_lossy(&self.buffer).trim().to_string();
            self.buffer.clear();
            if !line.is_empty() {
                out.push(SseEvent {
                    event: None,
                    data: line,
                    id: None,
                });
            }
        }
        out
    }

    pub fn finish(&mut self) -> Vec<SseEvent> {
        let tail = std::mem::take(&mut self.buffer);
        let decoded = String::from_utf8_lossy(&tail);
        let trimmed = decoded.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![SseEvent {
                event: None,
                data: trimmed.to_string(),
                id: None,
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_survives_fragmentation() {
        let mut sse = SseParser::new();
        let mut ndjson = NdjsonParser::new();
        let sse_events: Vec<_> = "data: 東京\n\n"
            .as_bytes()
            .chunks(1)
            .flat_map(|b| sse.push(b))
            .collect();
        let ndjson_events: Vec<_> = "\"東京\"\n"
            .as_bytes()
            .chunks(1)
            .flat_map(|b| ndjson.push(b))
            .collect();
        assert_eq!(sse_events[0].data, "東京");
        assert_eq!(ndjson_events[0].data, "\"東京\"");
    }
}
