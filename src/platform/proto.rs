#![allow(dead_code)]
//! Protobuf types for Feishu/Lark WebSocket protocol (pbbp2).
//! Manually generated from pbbp2.proto to avoid protoc dependency.

use bytes::{Buf, BufMut};

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Header {
    pub fn encode(&self, buf: &mut impl BufMut) {
        // field 1: key, wire type 2 (length-delimited)
        buf.put_u8((1 << 3) | 2);
        encode_varint(buf, self.key.len() as u64);
        buf.put_slice(self.key.as_bytes());
        // field 2: value, wire type 2 (length-delimited)
        buf.put_u8((2 << 3) | 2);
        encode_varint(buf, self.value.len() as u64);
        buf.put_slice(self.value.as_bytes());
    }

    #[allow(dead_code)]
    pub fn decode(_buf: &mut impl Buf) -> Option<Self> {
        // Header::decode is not used directly; Frame::decode uses decode_header() on slices.
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub seq_id: u64,
    pub log_id: u64,
    pub service: i32,
    pub method: i32,
    pub headers: Vec<Header>,
    pub payload_encoding: Option<String>,
    pub payload_type: Option<String>,
    pub payload: Option<Vec<u8>>,
    pub log_id_new: Option<String>,
}

impl Frame {
    pub fn encode(&self, buf: &mut impl BufMut) {
        // field 1: SeqID (varint)
        buf.put_u8(1 << 3);
        encode_varint(buf, self.seq_id);
        // field 2: LogID (varint)
        buf.put_u8(2 << 3);
        encode_varint(buf, self.log_id);
        // field 3: service (varint)
        buf.put_u8(3 << 3);
        encode_varint(buf, self.service as u64);
        // field 4: method (varint)
        buf.put_u8(4 << 3);
        encode_varint(buf, self.method as u64);
        // field 5: headers (repeated, length-delimited)
        for h in &self.headers {
            buf.put_u8((5 << 3) | 2);
            let mut hbuf = Vec::new();
            h.encode(&mut hbuf);
            encode_varint(buf, hbuf.len() as u64);
            buf.put_slice(&hbuf);
        }
        // field 6: payload_encoding
        if let Some(ref v) = self.payload_encoding {
            buf.put_u8((6 << 3) | 2);
            encode_varint(buf, v.len() as u64);
            buf.put_slice(v.as_bytes());
        }
        // field 7: payload_type
        if let Some(ref v) = self.payload_type {
            buf.put_u8((7 << 3) | 2);
            encode_varint(buf, v.len() as u64);
            buf.put_slice(v.as_bytes());
        }
        // field 8: payload
        if let Some(ref v) = self.payload {
            buf.put_u8((8 << 3) | 2);
            encode_varint(buf, v.len() as u64);
            buf.put_slice(v);
        }
        // field 9: LogIDNew
        if let Some(ref v) = self.log_id_new {
            buf.put_u8((9 << 3) | 2);
            encode_varint(buf, v.len() as u64);
            buf.put_slice(v.as_bytes());
        }
    }

    pub fn decode(mut buf: &[u8]) -> Option<Self> {
        let mut frame = Frame {
            seq_id: 0,
            log_id: 0,
            service: 0,
            method: 0,
            headers: Vec::new(),
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        };

        while !buf.is_empty() {
            let (field_number, wire_type, consumed) = decode_tag(buf)?;
            buf = &buf[consumed..];

            match (field_number, wire_type) {
                (1, 0) => {
                    let (v, c) = decode_varint(buf)?;
                    frame.seq_id = v;
                    buf = &buf[c..];
                }
                (2, 0) => {
                    let (v, c) = decode_varint(buf)?;
                    frame.log_id = v;
                    buf = &buf[c..];
                }
                (3, 0) => {
                    let (v, c) = decode_varint(buf)?;
                    frame.service = v as i32;
                    buf = &buf[c..];
                }
                (4, 0) => {
                    let (v, c) = decode_varint(buf)?;
                    frame.method = v as i32;
                    buf = &buf[c..];
                }
                (5, 2) => {
                    let (len, c) = decode_varint(buf)?;
                    let len = len as usize;
                    buf = &buf[c..];
                    if buf.len() < len {
                        return None;
                    }
                    if let Some(h) = decode_header(&buf[..len]) {
                        frame.headers.push(h);
                    }
                    buf = &buf[len..];
                }
                (6, 2) => {
                    let (len, c) = decode_varint(buf)?;
                    let len = len as usize;
                    buf = &buf[c..];
                    if buf.len() < len {
                        return None;
                    }
                    frame.payload_encoding = Some(String::from_utf8_lossy(&buf[..len]).to_string());
                    buf = &buf[len..];
                }
                (7, 2) => {
                    let (len, c) = decode_varint(buf)?;
                    let len = len as usize;
                    buf = &buf[c..];
                    if buf.len() < len {
                        return None;
                    }
                    frame.payload_type = Some(String::from_utf8_lossy(&buf[..len]).to_string());
                    buf = &buf[len..];
                }
                (8, 2) => {
                    let (len, c) = decode_varint(buf)?;
                    let len = len as usize;
                    buf = &buf[c..];
                    if buf.len() < len {
                        return None;
                    }
                    frame.payload = Some(buf[..len].to_vec());
                    buf = &buf[len..];
                }
                (9, 2) => {
                    let (len, c) = decode_varint(buf)?;
                    let len = len as usize;
                    buf = &buf[c..];
                    if buf.len() < len {
                        return None;
                    }
                    frame.log_id_new = Some(String::from_utf8_lossy(&buf[..len]).to_string());
                    buf = &buf[len..];
                }
                _ => {
                    // skip unknown field
                    match wire_type {
                        0 => {
                            let (_, c) = decode_varint(buf)?;
                            buf = &buf[c..];
                        }
                        2 => {
                            let (len, c) = decode_varint(buf)?;
                            let len = len as usize;
                            buf = &buf[c..];
                            if buf.len() < len {
                                return None;
                            }
                            buf = &buf[len..];
                        }
                        1 => {
                            if buf.len() < 8 {
                                return None;
                            }
                            buf = &buf[8..];
                        }
                        5 => {
                            if buf.len() < 4 {
                                return None;
                            }
                            buf = &buf[4..];
                        }
                        _ => return None,
                    }
                }
            }
        }

        Some(frame)
    }
}

pub(crate) fn encode_varint(buf: &mut impl BufMut, mut value: u64) {
    while value >= 0x80 {
        buf.put_u8((value as u8) | 0x80);
        value >>= 7;
    }
    buf.put_u8(value as u8);
}

pub(crate) fn decode_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut i = 0;
    while i < buf.len() && i < 10 {
        let byte = buf[i];
        result |= ((byte & 0x7f) as u64) << shift;
        i += 1;
        if byte & 0x80 == 0 {
            return Some((result, i));
        }
        shift += 7;
    }
    None
}

pub(crate) fn decode_tag(buf: &[u8]) -> Option<(u32, u32, usize)> {
    let (tag, consumed) = decode_varint(buf)?;
    let field_number = (tag >> 3) as u32;
    let wire_type = (tag & 0x7) as u32;
    Some((field_number, wire_type, consumed))
}

/// Encode a Frame into a byte vector for WebSocket transmission.
pub(crate) fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut buf = Vec::new();
    frame.encode(&mut buf);
    buf
}

/// Try to decode a Frame from the beginning of a BytesMut buffer.
/// On success, advances the buffer past the decoded frame.
pub(crate) fn decode_frame(buf: &mut bytes::BytesMut) -> Option<Frame> {
    if buf.is_empty() {
        return None;
    }
    let frame = Frame::decode(&buf[..])?;
    // pbbp2 sends one frame per WebSocket binary message.
    let len = buf.len();
    buf.advance(len);
    Some(frame)
}

pub(crate) fn decode_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < 2 {
        return None;
    }
    let tag1 = buf[0];
    if tag1 != ((1 << 3) | 2) {
        return None;
    }
    let (key_len, c1) = decode_varint(&buf[1..])?;
    let key_len = key_len as usize;
    let start = 1 + c1;
    if buf.len() < start + key_len + 1 {
        return None;
    }
    let key = String::from_utf8_lossy(&buf[start..start + key_len]).to_string();
    let tag2_pos = start + key_len;
    let tag2 = buf[tag2_pos];
    if tag2 != ((2 << 3) | 2) {
        return None;
    }
    let (value_len, c2) = decode_varint(&buf[tag2_pos + 1..])?;
    let value_len = value_len as usize;
    let vstart = tag2_pos + 1 + c2;
    if buf.len() < vstart + value_len {
        return None;
    }
    let value = String::from_utf8_lossy(&buf[vstart..vstart + value_len]).to_string();
    Some(Header { key, value })
}
