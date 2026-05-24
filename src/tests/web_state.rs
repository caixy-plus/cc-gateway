use crate::web::state::{broadcast_deliver, broadcast_event};

#[test]
fn test_broadcast_event_received() {
    let mut rx = crate::web::state::EVENT_BUS.subscribe();

    broadcast_event("sid-1", "webui", "chat-1", "user", "hello");

    let event = rx.try_recv().expect("should receive event");
    assert_eq!(event.session_id, "sid-1");
    assert_eq!(event.platform, "webui");
    assert_eq!(event.chat_id, "chat-1");
    assert_eq!(event.role, "user");
    assert_eq!(event.content, "hello");
    assert!(!event.timestamp.is_empty());
}

#[test]
fn test_broadcast_event_multiple_subscribers() {
    let mut rx1 = crate::web::state::EVENT_BUS.subscribe();
    let mut rx2 = crate::web::state::EVENT_BUS.subscribe();

    broadcast_event("sid-2", "feishu", "chat-2", "assistant", "world");

    let e1 = rx1.try_recv().unwrap();
    let e2 = rx2.try_recv().unwrap();
    assert_eq!(e1.content, "world");
    assert_eq!(e2.content, "world");
}

#[test]
fn test_broadcast_deliver_received() {
    let mut rx = crate::web::state::DELIVER_BUS.subscribe();

    broadcast_deliver("sid-1", "/tmp/file.txt", Some("here"));

    let req = rx.try_recv().expect("should receive deliver request");
    assert_eq!(req.session_id, "sid-1");
    assert_eq!(req.path, "/tmp/file.txt");
    assert_eq!(req.message, Some("here".to_string()));
}

#[test]
fn test_broadcast_deliver_without_message() {
    let mut rx = crate::web::state::DELIVER_BUS.subscribe();

    broadcast_deliver("sid-1", "/tmp", None);

    let req = rx.try_recv().unwrap();
    assert_eq!(req.message, None);
}
