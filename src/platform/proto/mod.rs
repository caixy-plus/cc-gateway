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
        buf.put_u8((1 << 3) | 0);
        encode_varint(buf, self.seq_id);
        // field 2: LogID (varint)
        buf.put_u8((2 << 3) | 0);
        encode_varint(buf, self.log_id);
        // field 3: service (varint)
        buf.put_u8((3 << 3) | 0);
        encode_varint(buf, self.service as u64);
        // field 4: method (varint)
        buf.put_u8((4 << 3) | 0);
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

fn encode_varint(buf: &mut impl BufMut, mut value: u64) {
    while value >= 0x80 {
        buf.put_u8((value as u8) | 0x80);
        value >>= 7;
    }
    buf.put_u8(value as u8);
}

fn decode_varint(buf: &[u8]) -> Option<(u64, usize)> {
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

fn decode_tag(buf: &[u8]) -> Option<(u32, u32, usize)> {
    let (tag, consumed) = decode_varint(buf)?;
    let field_number = (tag >> 3) as u32;
    let wire_type = (tag & 0x7) as u32;
    Some((field_number, wire_type, consumed))
}

fn decode_header(buf: &[u8]) -> Option<Header> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_varint() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0);
        assert_eq!(decode_varint(&buf), Some((0, 1)));

        buf.clear();
        encode_varint(&mut buf, 1);
        assert_eq!(decode_varint(&buf), Some((1, 1)));

        buf.clear();
        encode_varint(&mut buf, 127);
        assert_eq!(decode_varint(&buf), Some((127, 1)));

        buf.clear();
        encode_varint(&mut buf, 128);
        assert_eq!(decode_varint(&buf), Some((128, 2)));

        buf.clear();
        encode_varint(&mut buf, 16383);
        assert_eq!(decode_varint(&buf), Some((16383, 2)));

        buf.clear();
        encode_varint(&mut buf, 16384);
        assert_eq!(decode_varint(&buf), Some((16384, 3)));

        buf.clear();
        encode_varint(&mut buf, u64::MAX);
        assert_eq!(decode_varint(&buf), Some((u64::MAX, 10)));
    }

    #[test]
    fn test_decode_varint_empty() {
        assert_eq!(decode_varint(b""), None);
    }

    #[test]
    fn test_decode_varint_incomplete() {
        // 10 bytes all with continuation bit set
        let buf = [0x80; 10];
        assert_eq!(decode_varint(&buf), None);
    }

    #[test]
    fn test_decode_tag() {
        // field 1, wire type 0 -> tag = (1 << 3) | 0 = 8
        assert_eq!(decode_tag(&[8]), Some((1, 0, 1)));
        // field 2, wire type 2 -> tag = (2 << 3) | 2 = 18
        assert_eq!(decode_tag(&[18]), Some((2, 2, 1)));
    }

    #[test]
    fn test_header_encode_decode() {
        let header = Header {
            key: "X-Test-Key".to_string(),
            value: "test-value".to_string(),
        };
        let mut buf = Vec::new();
        header.encode(&mut buf);
        let decoded = decode_header(&buf).unwrap();
        assert_eq!(decoded.key, "X-Test-Key");
        assert_eq!(decoded.value, "test-value");
    }

    #[test]
    fn test_header_decode_too_short() {
        assert!(decode_header(b"").is_none());
        assert!(decode_header(&[0x0a]).is_none());
    }

    #[test]
    fn test_header_decode_bad_tag() {
        // Wrong field number for first tag
        assert!(decode_header(&[0x12, 0x01, b'k']).is_none());
    }

    #[test]
    fn test_frame_encode_decode_roundtrip() {
        let frame = Frame {
            seq_id: 42,
            log_id: 100,
            service: 1,
            method: 2,
            headers: vec![
                Header { key: "k1".to_string(), value: "v1".to_string() },
                Header { key: "k2".to_string(), value: "v2".to_string() },
            ],
            payload_encoding: Some("json".to_string()),
            payload_type: Some("event".to_string()),
            payload: Some(vec![0x01, 0x02, 0x03]),
            log_id_new: Some("new-log-id".to_string()),
        };

        let mut buf = Vec::new();
        frame.encode(&mut buf);
        let decoded = Frame::decode(&buf).unwrap();

        assert_eq!(decoded.seq_id, 42);
        assert_eq!(decoded.log_id, 100);
        assert_eq!(decoded.service, 1);
        assert_eq!(decoded.method, 2);
        assert_eq!(decoded.headers.len(), 2);
        assert_eq!(decoded.headers[0].key, "k1");
        assert_eq!(decoded.headers[0].value, "v1");
        assert_eq!(decoded.headers[1].key, "k2");
        assert_eq!(decoded.headers[1].value, "v2");
        assert_eq!(decoded.payload_encoding, Some("json".to_string()));
        assert_eq!(decoded.payload_type, Some("event".to_string()));
        assert_eq!(decoded.payload, Some(vec![0x01, 0x02, 0x03]));
        assert_eq!(decoded.log_id_new, Some("new-log-id".to_string()));
    }

    #[test]
    fn test_frame_decode_empty() {
        assert!(Frame::decode(b"").is_some()); // Empty message decodes to default frame
    }

    #[test]
    fn test_frame_decode_unknown_field() {
        // Encode a frame with an unknown field (field 100, wire type 0, varint value 1)
        let mut buf = Vec::new();
        encode_varint(&mut buf, ((100 << 3) | 0) as u64);
        encode_varint(&mut buf, 1);
        let decoded = Frame::decode(&buf).unwrap();
        assert_eq!(decoded.seq_id, 0);
        assert_eq!(decoded.log_id, 0);
    }

    #[test]
    fn test_frame_decode_unknown_field_length_delimited() {
        // Unknown field 100, wire type 2, length 3
        let mut buf = Vec::new();
        encode_varint(&mut buf, ((100 << 3) | 2) as u64);
        encode_varint(&mut buf, 3);
        buf.extend_from_slice(b"abc");
        let decoded = Frame::decode(&buf).unwrap();
        assert_eq!(decoded.seq_id, 0);
    }

    #[test]
    fn test_frame_decode_unknown_field_64bit() {
        // Unknown field 100, wire type 1 (64-bit), 8 bytes
        let mut buf = Vec::new();
        encode_varint(&mut buf, ((100 << 3) | 1) as u64);
        buf.extend_from_slice(&[0u8; 8]);
        let decoded = Frame::decode(&buf).unwrap();
        assert_eq!(decoded.seq_id, 0);
    }

    #[test]
    fn test_frame_decode_unknown_field_32bit() {
        // Unknown field 100, wire type 5 (32-bit), 4 bytes
        let mut buf = Vec::new();
        encode_varint(&mut buf, ((100 << 3) | 5) as u64);
        buf.extend_from_slice(&[0u8; 4]);
        let decoded = Frame::decode(&buf).unwrap();
        assert_eq!(decoded.seq_id, 0);
    }

    #[test]
    fn test_frame_decode_truncated() {
        // Frame with field 1 but truncated varint
        let buf = [0x08, 0x80]; // tag for field 1, incomplete varint
        assert!(Frame::decode(&buf).is_none());
    }

    #[test]
    fn test_frame_decode_string_field_truncated() {
        // Frame with field 6 (payload_encoding) but truncated length-delimited
        let mut buf = Vec::new();
        encode_varint(&mut buf, ((6 << 3) | 2) as u64);
        encode_varint(&mut buf, 100); // says 100 bytes but only 2 follow
        buf.extend_from_slice(b"ab");
        assert!(Frame::decode(&buf).is_none());
    }

    #[test]
    fn test_header_encode_decode_empty() {
        let header = Header {
            key: "".to_string(),
            value: "".to_string(),
        };
        let mut buf = Vec::new();
        header.encode(&mut buf);
        let decoded = decode_header(&buf).unwrap();
        assert_eq!(decoded.key, "");
        assert_eq!(decoded.value, "");
    }
}



#[cfg(test)]
mod protobuf_compat_tests {
    use crate::platform::proto::Frame;

    fn hex_decode(hex: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chars = hex.chars();
        while let (Some(c1), Some(c2)) = (chars.next(), chars.next()) {
            let byte = u8::from_str_radix(&format!("{}{}", c1, c2), 16).unwrap();
            bytes.push(byte);
        }
        bytes
    }

    #[test]
    fn test_decode_python_ping_frame() {
        let hex = "0800100018f681801020002a0c0a0474797065120470696e67";
        let bytes = hex_decode(hex);
        let frame = Frame::decode(&bytes).expect("Should decode PING frame");
        assert_eq!(frame.seq_id, 0);
        assert_eq!(frame.log_id, 0);
        assert_eq!(frame.service, 33554678);
        assert_eq!(frame.method, 0);
        assert_eq!(frame.headers.len(), 1);
        assert_eq!(frame.headers[0].key, "type");
        assert_eq!(frame.headers[0].value, "ping");
        println!("PING frame decoded OK");
    }

    #[test]
    fn test_decode_python_data_frame() {
        let hex = "08b96010b2920418f681801020012a0d0a047479706512056576656e742a1a0a0a6d6573736167655f6964120c746573742d6d73672d31323342107b2274657374223a202264617461227d";
        let bytes = hex_decode(hex);
        let frame = Frame::decode(&bytes).expect("Should decode DATA frame");
        assert_eq!(frame.seq_id, 12345);
        assert_eq!(frame.log_id, 67890);
        assert_eq!(frame.service, 33554678);
        assert_eq!(frame.method, 1);
        assert_eq!(frame.headers.len(), 2);
        assert_eq!(frame.headers[0].key, "type");
        assert_eq!(frame.headers[0].value, "event");
        assert_eq!(frame.headers[1].key, "message_id");
        assert_eq!(frame.headers[1].value, "test-msg-123");
        let payload = frame.payload.as_ref().map(|v| String::from_utf8_lossy(v).to_string());
        assert_eq!(payload, Some("{\"test\": \"data\"}".to_string()));
        println!("DATA frame decoded OK");
    }
}


#[cfg(test)]
mod ack_compat_tests {
    use crate::platform::proto::{Frame, Header};

    fn hex_decode(hex: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chars = hex.chars();
        while let (Some(c1), Some(c2)) = (chars.next(), chars.next()) {
            let byte = u8::from_str_radix(&format!("{}{}", c1, c2), 16).unwrap();
            bytes.push(byte);
        }
        bytes
    }

    #[test]
    fn test_ack_frame_matches_official_sdk() {
        // Original DATA frame from official SDKs
        let data_hex = "08b96010b2920418f681801020012a0d0a047479706512056576656e742a1a0a0a6d6573736167655f6964120c746573742d6d73672d31323342107b2274657374223a202264617461227d";
        let data_bytes = hex_decode(data_hex);
        let mut frame = Frame::decode(&data_bytes).expect("Should decode DATA frame");

        // Official SDKs keep the original DATA method (1) for ACK.
        // Do NOT change to CONTROL (0) — that breaks event delivery.
        assert_eq!(frame.method, 1);

        // Modify frame like official SDKs do: set Response payload and add biz_rt header.
        frame.payload = Some(br#"{"code":200,"headers":{},"data":null}"#.to_vec());
        frame.headers.push(Header { key: "biz_rt".to_string(), value: "5".to_string() });

        // Encode
        let mut buf = bytes::BytesMut::new();
        frame.encode(&mut buf);
        let ack_bytes = buf.freeze();

        println!("ACK frame hex: {}", ack_bytes.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
        println!("ACK frame length: {}", ack_bytes.len());

        // Verify it can be decoded back
        let decoded = Frame::decode(&ack_bytes).expect("Should decode ACK frame");
        assert_eq!(decoded.seq_id, 12345);
        assert_eq!(decoded.log_id, 67890);
        assert_eq!(decoded.method, 1); // MUST remain DATA (1)
        assert_eq!(decoded.headers.len(), 3); // type, message_id, biz_rt

        let payload_str = String::from_utf8_lossy(decoded.payload.as_ref().unwrap());
        assert_eq!(payload_str, r#"{"code":200,"headers":{},"data":null}"#);

        println!("ACK frame matches official SDK behavior!");
    }
}
