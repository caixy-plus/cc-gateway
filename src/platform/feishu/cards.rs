use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::{FeishuPlatform, MentionInfo, NormalizedMessage, PendingPermissionContext};
use crate::{t, t_fmt};

impl FeishuPlatform {
    /// Build an interactive approval card for Claude tool permission requests.
    /// Returns a Feishu card protocol v2 JSON object.
    pub(crate) fn build_permission_card(
        &self,
        request_id: &str,
        tool_name: &str,
        tool_input: Option<&Value>,
    ) -> Value {
        let input_preview = tool_input
            .and_then(|v| serde_json::to_string_pretty(v).ok())
            .unwrap_or_else(|| "{}".to_string());
        // Truncate if too long
        let input_preview = if input_preview.len() > 500 {
            format!("{}...", &input_preview[..500])
        } else {
            input_preview
        };

        json!({
            "schema": "2.0",
            "config": {
                "style": {
                    "text_size": {
                        "level1": 17,
                        "level2": 16,
                        "level3": 14
                    }
                }
            },
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": t!("feishu.permission_title")
                },
                "subtitle": {
                    "tag": "plain_text",
                    "content": t_fmt!("feishu.permission_subtitle", NAME = tool_name)
                },
                "template": "indigo"
            },
            "body": {
                "elements": [
                    {
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": t_fmt!("feishu.request_id_label", ID = request_id)
                        }
                    },
                    {
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": t_fmt!("feishu.tool_input_label", INPUT = input_preview)
                        }
                    },
                    {
                        "tag": "hr"
                    },
                    {
                        "tag": "action",
                        "layout": "default",
                        "actions": [
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_once")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_once",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_session")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_session",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_always")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_always",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.deny")
                                },
                                "type": "danger",
                                "value": {
                                    "action": "deny",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            }
                        ]
                    }
                ]
            }
        })
    }

    /// Build an interactive single-select card.
    /// If options.len() > 5, uses select_static; otherwise uses primary buttons.
    pub(crate) fn build_single_select_card(
        &self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Value {
        let title = if prompt.len() > 80 {
            format!("{}...", &prompt[..80])
        } else {
            prompt.to_string()
        };

        let elements = if options.len() > 5 {
            let select_options: Vec<Value> = options
                .iter()
                .map(|opt| {
                    json!({
                        "text": {
                            "tag": "plain_text",
                            "content": opt
                        },
                        "value": opt
                    })
                })
                .collect();
            vec![
                json!({
                    "tag": "div",
                    "text": {
                        "tag": "lark_md",
                        "content": title
                    }
                }),
                json!({
                    "tag": "action",
                    "layout": "default",
                    "actions": [
                        {
                            "tag": "select_static",
                            "placeholder": {
                                "tag": "plain_text",
                                "content": "请选择..."
                            },
                            "options": select_options,
                            "value": {
                                "action": "select",
                                "request_id": request_id
                            }
                        }
                    ]
                }),
            ]
        } else {
            let buttons: Vec<Value> = options
                .iter()
                .map(|opt| {
                    json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": opt
                        },
                        "type": "primary",
                        "value": {
                            "action": "select",
                            "request_id": request_id,
                            "answer": opt
                        }
                    })
                })
                .collect();
            vec![
                json!({
                    "tag": "div",
                    "text": {
                        "tag": "lark_md",
                        "content": title
                    }
                }),
                json!({
                    "tag": "action",
                    "layout": "default",
                    "actions": buttons
                }),
            ]
        };

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": "请选择"
                },
                "template": "blue"
            },
            "body": {
                "elements": elements
            }
        })
    }

    /// Build a multi-select card using buttons for each option.
    /// Each click updates the selection state (stored in interaction_store externally).
    pub(crate) fn build_multi_select_card(
        &self,
        request_id: &str,
        prompt: &str,
        options: &[String],
        selected: &[String],
    ) -> Value {
        let title = if prompt.len() > 80 {
            format!("{}...", &prompt[..80])
        } else {
            prompt.to_string()
        };

        let buttons: Vec<Value> = options
            .iter()
            .map(|opt| {
                let is_selected = selected.contains(opt);
                let label = if is_selected {
                    format!("✅ {}", opt)
                } else {
                    opt.clone()
                };
                let btn_type = if is_selected { "default" } else { "primary" };
                let mut new_selected = selected.to_vec();
                if is_selected {
                    new_selected.retain(|s| s != opt);
                } else {
                    new_selected.push(opt.clone());
                }
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": label
                    },
                    "type": btn_type,
                    "value": {
                        "action": "toggle_select",
                        "request_id": request_id,
                        "toggle": opt,
                        "selected": new_selected
                    }
                })
            })
            .collect();

        let actions = vec![
            json!({
                "tag": "action",
                "layout": "default",
                "actions": buttons
            }),
            json!({
                "tag": "action",
                "layout": "default",
                "actions": [
                    {
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": "提交"
                        },
                        "type": "primary",
                        "value": {
                            "action": "submit_multi",
                            "request_id": request_id
                        }
                    },
                    {
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": "取消"
                        },
                        "type": "danger",
                        "value": {
                            "action": "cancel_multi",
                            "request_id": request_id
                        }
                    }
                ]
            }),
        ];

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": title
                },
                "template": "blue"
            },
            "body": {
                "elements": actions
            }
        })
    }

    /// Build a text-input hint card prompting the user to reply with a text message.
    pub(crate) fn build_text_input_hint_card(
        &self,
        request_id: &str,
        prompt: &str,
    ) -> Value {
        let title = if prompt.len() > 80 {
            format!("{}...", &prompt[..80])
        } else {
            prompt.to_string()
        };

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": title
                },
                "template": "wathet"
            },
            "body": {
                "elements": [
                    {
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": "请直接回复本条消息，输入你的回答。"
                        }
                    },
                    {
                        "tag": "action",
                        "layout": "default",
                        "actions": [
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": "取消"
                                },
                                "type": "danger",
                                "value": {
                                    "action": "cancel_text_input",
                                    "request_id": request_id
                                }
                            }
                        ]
                    }
                ]
            }
        })
    }

    /// Build a confirm/deny card with two buttons.
    pub(crate) fn build_confirm_card(
        &self,
        request_id: &str,
        prompt: &str,
    ) -> Value {
        let title = if prompt.len() > 80 {
            format!("{}...", &prompt[..80])
        } else {
            prompt.to_string()
        };

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": title
                },
                "template": "orange"
            },
            "body": {
                "elements": [
                    {
                        "tag": "action",
                        "layout": "default",
                        "actions": [
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": "确认"
                                },
                                "type": "primary",
                                "value": {
                                    "action": "confirm",
                                    "request_id": request_id,
                                    "answer": true
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": "取消"
                                },
                                "type": "danger",
                                "value": {
                                    "action": "confirm",
                                    "request_id": request_id,
                                    "answer": false
                                }
                            }
                        ]
                    }
                ]
            }
        })
    }

    /// Build an interactive session history card.
    /// Feishu card schema v2.
    pub(crate) fn build_session_history_card(
        &self,
        sessions: &[crate::session::model::Session],
        receive_id_type: &str,
        receive_id: &str,
    ) -> Value {
        let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let mut elements: Vec<Value> = Vec::new();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": t!("feishu.session_history_subtitle")
            }
        }));

        for session in sessions {
            let status_dot = if session.active { "🟢" } else { "⚪" };
            let time = session.created_at
                .with_timezone(&china_tz)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            let mut info_parts = vec![
                format!("**{}** {}", status_dot, session.title),
                format!("📁 {}", session.work_dir),
                format!("🕒 {}", time),
            ];
            if let Some(ref csid) = session.claude_session_id {
                info_parts.push(format!("🔑 `{}`", csid));
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
                    "content": t!("feishu.resume")
                },
                "type": "primary",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "resume",
                            "session_id": session.id,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
                        }
                    }
                ]
            }));
            elements.push(json!({
                "tag": "button",
                "text": {
                    "tag": "plain_text",
                    "content": t!("feishu.start_new_session")
                },
                "type": "default",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "resume",
                            "session_id": "",
                            "work_dir": session.work_dir,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
                        }
                    }
                ]
            }));
            elements.push(json!({
                "tag": "button",
                "text": {
                    "tag": "plain_text",
                    "content": t!("feishu.delete_session")
                },
                "type": "danger",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "delete_session",
                            "session_id": session.id,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
                        }
                    }
                ]
            }));
            elements.push(json!({ "tag": "hr" }));
        }

        // Remove trailing hr if present
        if elements.last().and_then(|e| e.get("tag")).and_then(|v| v.as_str()) == Some("hr") {
            elements.pop();
        }

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": t!("feishu.session_history_title")
                },
                "template": "indigo"
            },
            "body": {
                "elements": elements
            }
        })
    }

    /// Build an interactive directory selection card.
    /// Feishu card schema v2: buttons are placed directly in body.elements.
    pub(crate) fn build_dir_select_card(
        &self,
        dirs: &[(String, String)],
        page: usize,
        dir: &str,
        receive_id_type: &str,
        receive_id: &str,
    ) -> Value {
        let mut elements: Vec<Value> = Vec::new();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": t!("feishu.choose_dir")
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
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
                        }
                    }
                ]
            }));
        }

        // Pagination controls
        let mut pagination_buttons: Vec<Value> = Vec::new();
        if page > 0 {
            pagination_buttons.push(json!({
                "tag": "button",
                "text": {
                    "tag": "plain_text",
                    "content": t!("feishu.prev_page")
                },
                "type": "default",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "ll_page",
                            "page": page - 1,
                            "dir": dir,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
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
                    "content": t!("feishu.next_page")
                },
                "type": "default",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "ll_page",
                            "page": page + 1,
                            "dir": dir,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
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
                    "content": t_fmt!("feishu.page_info", PAGE = page + 1, TOTAL = (dirs.len() + MAX_DIRS - 1) / MAX_DIRS)
                }
            }));
            elements.push(json!({
                "tag": "action",
                "actions": pagination_buttons
            }));
        }

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": t!("feishu.select_dir_title")
                },
                "template": "indigo"
            },
            "body": {
                "elements": elements
            }
        })
    }

    /// List directory names under the given path.
    /// Store a pending permission context so card callbacks can be matched to requests.
    pub(crate) fn store_pending_permission(&self, ctx: PendingPermissionContext) {
        self.pending_permissions
            .insert(ctx.request_id.clone(), ctx);
    }

    /// Retrieve and remove a pending permission context by request_id.
    pub(crate) fn take_pending_permission(&self, request_id: &str) -> Option<PendingPermissionContext> {
        self.pending_permissions.remove(request_id).map(|(_, v)| v)
    }

    /// Clean up expired pending permissions (older than 10 minutes).
    pub(crate) fn cleanup_pending_permissions(&self) {
        let now = Instant::now();
        let max_age = Duration::from_secs(600);
        self.pending_permissions
            .retain(|_, v| now.duration_since(v.created_at) < max_age);
    }

    /// Normalize a raw Feishu event JSON into a structured message.
    pub(crate) fn normalize_message(&self, event_json: &Value) -> Option<NormalizedMessage> {
        let event = event_json.get("event")?;
        let message = event.get("message")?;

        let message_id = message
            .get("message_id")?
            .as_str()?
            .to_string();

        // Deduplicate by message_id — check early so we short-circuit
        // on known duplicates, but defer the insert until ALL field
        // extractions succeed so a partial failure does not poison the
        // cache for a subsequent retransmission.
        if self.dedup_cache.contains(&message_id) {
            return None;
        }

        let message_type = message
            .get("message_type")?
            .as_str()?
            .to_string();

        let content = message
            .get("content")?
            .as_str()?
            .to_string();

        let sender = event.get("sender")?;
        let sender_id = sender.get("sender_id")?;
        let sender_open_id = sender_id
            .get("open_id")?
            .as_str()?
            .to_string();
        let sender_name = sender
            .get("sender_type")?
            .as_str()
            .map(|s| s.to_string());

        let chat_id = message
            .get("chat_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let chat_type = message
            .get("chat_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract mentions
        let mut mentions = Vec::new();
        if let Some(mentions_arr) = message.get("mentions").and_then(|v| v.as_array()) {
            for m in mentions_arr {
                let open_id = m
                    .get("id")
                    .and_then(|v| v.get("open_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let name = m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let key = m
                    .get("key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(oid) = open_id {
                    mentions.push(MentionInfo {
                        open_id: oid,
                        name,
                        key,
                    });
                }
            }
        }

        let (receive_id_type, receive_id) = if chat_type.as_deref() == Some("p2p") {
            ("open_id".to_string(), sender_open_id.clone())
        } else {
            ("chat_id".to_string(), chat_id.clone().unwrap_or_default())
        };

        // All field extractions succeeded — now insert into dedup cache.
        self.dedup_cache.insert(message_id.clone());

        Some(NormalizedMessage {
            message_id,
            message_type,
            content,
            sender_open_id,
            sender_name,
            chat_id,
            chat_type,
            mentions,
            raw: event_json.clone(),
            receive_id_type,
            receive_id,
        })
    }

}
