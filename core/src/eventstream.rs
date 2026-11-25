use serde_json::{json, Value};

use crate::{errors::NativeError, sse::SseEvent};

pub(crate) const MEDIA_TYPE: &str = "application/vnd.amazon.eventstream";

#[derive(Default, Debug)]
pub(crate) struct EventStream {
    pending: Vec<u8>,
}

impl EventStream {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, NativeError> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut remaining = self.pending.as_slice();
        while remaining.len() >= 12 {
            let total = u32_at(remaining, 0) as usize;
            let headers = u32_at(remaining, 4) as usize;
            if crc32(&remaining[..8]) != u32_at(remaining, 8) {
                return Err(stream_error("invalid prelude checksum"));
            }
            if headers > 128 * 1024
                || total < 16 + headers
                || total - 16 - headers > 16 * 1024 * 1024
            {
                return Err(stream_error("invalid frame size"));
            }
            if remaining.len() < total {
                break;
            }
            let frame = &remaining[..total - 4];
            if crc32(frame) != u32_at(remaining, total - 4) {
                return Err(stream_error("invalid message checksum"));
            }
            events.push(decode(&frame[12..12 + headers], &frame[12 + headers..])?);
            remaining = &remaining[total..];
        }
        let consumed = self.pending.len() - remaining.len();
        self.pending.drain(..consumed);
        Ok(events)
    }

    pub(crate) fn finish(&self) -> Result<(), NativeError> {
        if !self.pending.is_empty() {
            return Err(stream_error("truncated frame"));
        }
        Ok(())
    }
}

fn decode(mut headers: &[u8], payload: &[u8]) -> Result<SseEvent, NativeError> {
    let mut kind = None;
    let mut name = None;
    while !headers.is_empty() {
        let name_len = headers[0] as usize;
        let (key, rest) = headers[1..]
            .split_at_checked(name_len)
            .ok_or_else(|| stream_error("truncated header"))?;
        let (&tag, mut rest) = rest
            .split_first()
            .ok_or_else(|| stream_error("missing header type"))?;
        let len = match tag {
            0 | 1 => 0,
            2 => 1,
            3 => 2,
            4 => 4,
            5 | 8 => 8,
            9 => 16,
            6 | 7 if rest.len() >= 2 => {
                let len = u16::from_be_bytes([rest[0], rest[1]]);
                rest = &rest[2..];
                usize::from(len)
            }
            _ => return Err(stream_error("invalid header type")),
        };
        let (value, remainder) = rest
            .split_at_checked(len)
            .ok_or_else(|| stream_error("truncated header value"))?;
        headers = remainder;
        let field = match key {
            b":message-type" => &mut kind,
            b":event-type" => &mut name,
            _ => continue,
        };
        if tag != 7 || field.is_some() {
            return Err(stream_error("invalid event header"));
        }
        *field = Some(
            std::str::from_utf8(value)
                .map_err(|_| stream_error("invalid event header encoding"))?,
        );
    }
    match kind {
        Some("event") => {}
        Some("error" | "exception") => return Err(stream_error("upstream sent an error frame")),
        _ => return Err(stream_error("missing or invalid message type")),
    }
    let name = name
        .filter(|name| !name.is_empty())
        .ok_or_else(|| stream_error("missing event type"))?;
    let body: Value =
        serde_json::from_slice(payload).map_err(|_| stream_error("invalid event JSON"))?;
    Ok(SseEvent {
        event: Some(name.to_owned()),
        data: json!({name: body}).to_string(),
        id: None,
    })
}

fn stream_error(message: &str) -> NativeError {
    NativeError::StreamParse(format!("Amazon Event Stream: {message}"))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fragmented_frames_and_rejects_truncation_and_corruption() {
        let fixture = include_bytes!("../../conformance/native/aws_bedrock_stream.eventstream");
        let mut parser = EventStream::default();
        let mut events = Vec::new();
        for chunk in fixture.chunks(3) {
            events.extend(parser.push(chunk).unwrap());
        }
        parser.finish().unwrap();
        assert_eq!(events.len(), 8);
        assert!(events[4].data.contains("東京"));
        let mut parser = EventStream::default();
        parser.push(&fixture[..fixture.len() - 1]).unwrap();
        assert!(parser
            .finish()
            .unwrap_err()
            .to_string()
            .contains("truncated"));
        let mut corrupt = fixture.to_vec();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(EventStream::default()
            .push(&corrupt)
            .unwrap_err()
            .to_string()
            .contains("checksum"));
        let exception = include_bytes!("../../conformance/native/aws_bedrock_error.eventstream");
        assert!(EventStream::default()
            .push(exception)
            .unwrap_err()
            .to_string()
            .contains("error frame"));
    }
}
