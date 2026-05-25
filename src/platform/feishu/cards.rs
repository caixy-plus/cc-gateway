use serde_json::{json, Value};

/// Build an interactive directory-picker card with pagination.
/// Matches main branch `build_dir_select_card` format — Card v2.0 schema,
/// buttons in body.elements with `behaviors` callbacks.
pub fn build_dir_picker_card(
    dirs: &[(String, String)],
    page: usize,
    dir: &str,
    chat_id: &str,
    receive_id_type: &str,
    receive_id: &str,
) -> Value {
    let mut elements: Vec<Value> = Vec::new();
    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": crate::t!("feishu.choose_dir")
        }
    }));

    const MAX_DIRS: usize = 40;
    let start = page * MAX_DIRS;
    let end = ((page + 1) * MAX_DIRS).min(dirs.len());
    let page_dirs = &dirs[start..end];

    for (name, path) in page_dirs {
        elements.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": name
            },
            "type": "primary",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "cd",
                        "path": path,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
    }

    let mut pagination_buttons: Vec<Value> = Vec::new();
    if page > 0 {
        pagination_buttons.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": crate::t!("feishu.prev_page")
            },
            "type": "default",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "ll_page",
                        "page": page - 1,
                        "dir": dir,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
    }
    if end < dirs.len() {
        pagination_buttons.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": crate::t!("feishu.next_page")
            },
            "type": "default",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "ll_page",
                        "page": page + 1,
                        "dir": dir,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
    }
    if !pagination_buttons.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": crate::t_fmt!("feishu.page_info",
                    PAGE = page + 1,
                    TOTAL = (dirs.len() + MAX_DIRS - 1) / MAX_DIRS)
            }
        }));
        elements.extend(pagination_buttons);
    }

    json!({
        "schema": "2.0",
        "header": {
            "title": {
                "tag": "plain_text",
                "content": crate::t!("feishu.select_dir_title")
            },
            "template": "indigo"
        },
        "body": {
            "elements": elements
        }
    })
}

/// Build a session-history card with resume / new-session / delete buttons.
/// Matches main branch `build_session_history_card`.
pub fn build_session_history_card(
    sessions: &[crate::session::channel_model::ClaudeSession],
    chat_id: &str,
    receive_id_type: &str,
    receive_id: &str,
) -> Value {
    let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let mut elements: Vec<Value> = Vec::new();
    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": crate::t!("feishu.session_history_subtitle")
        }
    }));

    for session in sessions {
        let status_dot = if session.active {
            "\u{1F7E2}"
        } else {
            "\u{26AA}"
        };
        let time = session
            .created_at
            .with_timezone(&china_tz)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let mut info_parts = vec![
            format!("**{}** {}", status_dot, session.title),
            format!("\u{1F4C1} {}", session.work_dir),
            format!("\u{1F552} {}", time),
        ];
        if let Some(ref csid) = session.claude_session_id {
            info_parts.push(format!("\u{1F511} `{}`", csid));
        }
        let info_text = info_parts.join("\n");

        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": info_text
            }
        }));
        elements.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": crate::t!("feishu.resume")
            },
            "type": "primary",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "resume",
                        "session_id": session.id,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
        elements.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": crate::t!("feishu.start_new_session")
            },
            "type": "default",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "resume",
                        "session_id": "",
                        "work_dir": session.work_dir,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
        elements.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": crate::t!("feishu.delete_session")
            },
            "type": "danger",
            "behaviors": [
                {
                    "type": "callback",
                    "value": {
                        "cmd": "delete_session",
                        "session_id": session.id,
                        "chat_id": chat_id,
                        "receive_id_type": receive_id_type,
                        "receive_id": receive_id
                    }
                }
            ]
        }));
        elements.push(json!({ "tag": "hr" }));
    }

    if elements
        .last()
        .and_then(|e| e.get("tag"))
        .and_then(|v| v.as_str())
        == Some("hr")
    {
        elements.pop();
    }

    json!({
        "schema": "2.0",
        "header": {
            "title": {
                "tag": "plain_text",
                "content": crate::t!("feishu.session_history_title")
            },
            "template": "indigo"
        },
        "body": {
            "elements": elements
        }
    })
}

/// Build a permission-request card for Feishu with Allow/Deny buttons.
pub fn build_permission_card(request_id: &str, tool_name: &str, chat_id: &str) -> Value {
    json!({
        "schema": "2.0",
        "header": {
            "title": {
                "tag": "plain_text",
                "content": crate::t!("feishu.permission_title")
            },
            "template": "indigo"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": {
                        "tag": "lark_md",
                        "content": format!("{}\nID: `{}`", tool_name, request_id)
                    }
                },
                {
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": crate::t!("feishu.allow_button")
                    },
                    "type": "primary",
                    "behaviors": [
                        {
                            "type": "callback",
                            "value": {
                                "cmd": "allow",
                                "request_id": request_id,
                                "chat_id": chat_id
                            }
                        }
                    ]
                },
                {
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": crate::t!("feishu.deny_button")
                    },
                    "type": "danger",
                    "behaviors": [
                        {
                            "type": "callback",
                            "value": {
                                "cmd": "deny",
                                "request_id": request_id,
                                "chat_id": chat_id
                            }
                        }
                    ]
                }
            ]
        }
    })
}

/// Build a single-select / confirm card with option buttons.
pub fn build_select_card(
    request_id: &str,
    prompt: &str,
    options: &[String],
    chat_id: &str,
) -> Value {
    let option_buttons: Vec<Value> = options
        .iter()
        .map(|opt| {
            json!({
                "tag": "button",
                "text": {
                    "tag": "plain_text",
                    "content": opt
                },
                "type": "primary",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "select",
                            "request_id": request_id,
                            "option": opt,
                            "chat_id": chat_id
                        }
                    }
                ]
            })
        })
        .collect();

    let mut elements: Vec<Value> = vec![json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": prompt
        }
    })];
    elements.extend(option_buttons);

    json!({
        "schema": "2.0",
        "header": {
            "title": {
                "tag": "plain_text",
                "content": crate::t!("feishu.select_title")
            },
            "template": "indigo"
        },
        "body": {
            "elements": elements
        }
    })
}

/// Build a simple text card for Feishu (fallback).
pub fn build_text_card(title: &str, text: &str) -> Value {
    json!({
        "schema": "2.0",
        "header": {
            "title": {
                "tag": "plain_text",
                "content": title
            },
            "template": "indigo"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": {
                        "tag": "lark_md",
                        "content": text
                    }
                }
            ]
        }
    })
}
