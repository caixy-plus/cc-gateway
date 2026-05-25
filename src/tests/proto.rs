use crate::platform::feishu::{build_ack_frame, build_ping_frame, METHOD_CONTROL};
use crate::platform::proto::{decode_frame, encode_frame, Frame, Header};
use bytes::BytesMut;

#[test]
fn protobuf_frame_round_trips() {
    let frame = Frame {
        seq_id: 7,
        log_id: 9,
        service: 1,
        method: 2,
        headers: vec![Header {
            key: "type".to_string(),
            value: "PING".to_string(),
        }],
        payload_encoding: Some("json".to_string()),
        payload_type: Some("event".to_string()),
        payload: Some(br#"{"ok":true}"#.to_vec()),
        log_id_new: Some("log-new".to_string()),
    };

    let bytes = encode_frame(&frame);
    let mut bytes = BytesMut::from(bytes.as_slice());
    let decoded = decode_frame(&mut bytes).expect("frame should decode");

    assert_eq!(decoded, frame);
    assert!(bytes.is_empty());
}

#[test]
fn heartbeat_and_ack_frames_use_control_method() {
    let ping = build_ping_frame(42);
    let ack = build_ack_frame(&Frame {
        seq_id: 5,
        log_id: 0,
        service: 42,
        method: METHOD_CONTROL,
        headers: Vec::new(),
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    });

    assert_eq!(ping.method, METHOD_CONTROL);
    assert_eq!(ack.method, METHOD_CONTROL);
    assert_eq!(ack.seq_id, 5);
}
