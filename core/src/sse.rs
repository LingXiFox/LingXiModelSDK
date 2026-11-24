#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

/// Incremental line-oriented SSE parser handling arbitrary chunk boundaries.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    event_name: Option<String>,
    data_lines: Vec<String>,
    last_id: Option<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let mut line = self.buffer[..pos].to_string();
            self.buffer.drain(..=pos);
            if line.ends_with('\r') {
                line.pop();
            }
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
            let line = std::mem::take(&mut self.buffer);
            let line = line.strip_suffix('\r').unwrap_or(&line);
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
