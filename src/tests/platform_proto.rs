use crate::platform::proto::{
    decode_header, decode_tag, decode_varint, encode_varint, Frame, Header,
};

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
            Header {
                key: "k1".to_string(),
                value: "v1".to_string(),
            },
            Header {
                key: "k2".to_string(),
                value: "v2".to_string(),
            },
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
    let payload = frame
        .payload
        .as_ref()
        .map(|v| String::from_utf8_lossy(v).to_string());
    assert_eq!(payload, Some("{\"test\": \"data\"}".to_string()));
    println!("DATA frame decoded OK");
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
    frame.headers.push(Header {
        key: "biz_rt".to_string(),
        value: "5".to_string(),
    });

    // Encode
    let mut buf = bytes::BytesMut::new();
    frame.encode(&mut buf);
    let ack_bytes = buf.freeze();

    println!(
        "ACK frame hex: {}",
        ack_bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join("")
    );
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
