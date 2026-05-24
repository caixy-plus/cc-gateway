#[cfg(test)]
mod tests {
    use crate::config::model::FeishuConfig;
    use crate::config::model::GatewayConfig;
    use crate::platform::feishu::FeishuPlatform;
    use serde_json::json;

    /// Build a test FeishuPlatform. The config uses env-var placeholders so it
    /// does not require real credentials for card-building tests.
    fn test_platform() -> FeishuPlatform {
        let config = FeishuConfig {
            enabled: true,
            app_id: "${FEISHU_APP_ID}".to_string(),
            app_secret: "${FEISHU_APP_SECRET}".to_string(),
            allow_from: "*".to_string(),
            encrypt_key: "".to_string(),
            mode: "websocket".to_string(),
            webhook_bind: "0.0.0.0:3000".to_string(),
        };
        let gateway_config = GatewayConfig::default();
        FeishuPlatform::new(
            config,
            &gateway_config.default_dir,
            gateway_config.claude.clone(),
            gateway_config.show_thinking,
        )
    }

    // ------------------------------------------------------------------
    // build_permission_card
    // ------------------------------------------------------------------

    #[test]
    fn test_build_permission_card_structure() {
        let platform = test_platform();
        let card = platform.build_permission_card(
            "req-001",
            "Bash",
            Some(&json!({"command": "ls -la", "description": "List files"})),
        );
        // Top-level schema field
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        // Header fields
        assert!(card["header"]["title"]["content"].as_str().is_some());
        assert_eq!(card["header"]["template"].as_str(), Some("indigo"));
        // Body elements
        let elements = card["body"]["elements"].as_array().unwrap();
        // First div: request_id label, second div: tool input, then hr, then action with buttons
        assert!(elements.len() >= 4);
        // The action should contain four buttons: approve_once, approve_session, approve_always, deny
        let action = &elements[3];
        assert_eq!(action["tag"].as_str(), Some("action"));
        let actions = action["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 4);
    }

    #[test]
    fn test_build_permission_card_null_input() {
        let platform = test_platform();
        let card = platform.build_permission_card("req-002", "Read", None);
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        // Should still produce valid card with default input preview
        let elements = card["body"]["elements"].as_array().unwrap();
        let input_div = &elements[1];
        let content = input_div["text"]["content"].as_str().unwrap();
        // Default should be "{}" for null input
        assert!(content.contains("{}"));
    }

    #[test]
    fn test_build_permission_card_long_input_truncation() {
        let platform = test_platform();
        // Build an input string longer than 500 chars
        let long_value = format!("{}{}", "x".repeat(600), "TAIL");
        let card = platform.build_permission_card(
            "req-003",
            "Write",
            Some(&json!({"content": long_value})),
        );
        let elements = card["body"]["elements"].as_array().unwrap();
        let input_div = &elements[1];
        let content = input_div["text"]["content"].as_str().unwrap();
        // Should be truncated — long "x" prefix should be present but "TAIL" should not
        assert!(content.len() < 600);
        assert!(!content.contains("TAIL"));
    }

    // ------------------------------------------------------------------
    // card button action values
    // ------------------------------------------------------------------

    #[test]
    fn test_card_button_action_values() {
        let platform = test_platform();
        let card = platform.build_permission_card("req-btn-1", "WebSearch", None);
        let elements = card["body"]["elements"].as_array().unwrap();
        let action = &elements[3];
        let buttons = action["actions"].as_array().unwrap();

        // Approve-once button
        let btn_val = &buttons[0]["value"];
        assert_eq!(btn_val["action"].as_str(), Some("approve_once"));
        assert_eq!(btn_val["request_id"].as_str(), Some("req-btn-1"));
        assert_eq!(btn_val["tool_name"].as_str(), Some("WebSearch"));

        // Approve-session button
        let btn_val = &buttons[1]["value"];
        assert_eq!(btn_val["action"].as_str(), Some("approve_session"));
        assert_eq!(btn_val["request_id"].as_str(), Some("req-btn-1"));
        assert_eq!(btn_val["tool_name"].as_str(), Some("WebSearch"));

        // Approve-always button
        let btn_val = &buttons[2]["value"];
        assert_eq!(btn_val["action"].as_str(), Some("approve_always"));
        assert_eq!(btn_val["request_id"].as_str(), Some("req-btn-1"));
        assert_eq!(btn_val["tool_name"].as_str(), Some("WebSearch"));

        // Deny button
        let btn_val = &buttons[3]["value"];
        assert_eq!(btn_val["action"].as_str(), Some("deny"));
        assert_eq!(btn_val["request_id"].as_str(), Some("req-btn-1"));
        assert_eq!(btn_val["tool_name"].as_str(), Some("WebSearch"));
    }

    // ------------------------------------------------------------------
    // build_confirm_card
    // ------------------------------------------------------------------

    #[test]
    fn test_build_confirm_card_structure() {
        let platform = test_platform();
        let card = platform.build_confirm_card("req-conf-1", "Are you sure?");
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        assert_eq!(card["header"]["template"].as_str(), Some("orange"));
        // Title should be the prompt
        assert_eq!(
            card["header"]["title"]["content"].as_str(),
            Some("Are you sure?")
        );

        let elements = card["body"]["elements"].as_array().unwrap();
        let action = &elements[0];
        let buttons = action["actions"].as_array().unwrap();
        assert_eq!(buttons.len(), 2);

        // Confirm button
        assert_eq!(buttons[0]["value"]["action"].as_str(), Some("confirm"));
        assert_eq!(buttons[0]["value"]["answer"].as_bool(), Some(true));
        // Deny button
        assert_eq!(buttons[1]["value"]["action"].as_str(), Some("confirm"));
        assert_eq!(buttons[1]["value"]["answer"].as_bool(), Some(false));
    }

    #[test]
    fn test_build_confirm_card_prompt_truncation() {
        let platform = test_platform();
        let long_prompt = format!("{} and more", "x".repeat(100));
        let card = platform.build_confirm_card("req-conf-2", &long_prompt);
        let title = card["header"]["title"]["content"].as_str().unwrap();
        // Length should be <= 83 (80 + "...")
        assert!(title.len() <= 83);
        assert!(title.ends_with("..."));
        // The original text beyond 80 chars should not appear
        assert!(!title.contains("and more"));
    }

    // ------------------------------------------------------------------
    // build_single_select_card
    // ------------------------------------------------------------------

    #[test]
    fn test_build_single_select_card_few_options() {
        let platform = test_platform();
        let options: Vec<String> = vec![
            "Option A".into(),
            "Option B".into(),
            "Option C".into(),
        ];
        let card = platform.build_single_select_card("req-ss-1", "Pick one", &options);
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        assert_eq!(card["header"]["template"].as_str(), Some("blue"));

        let elements = card["body"]["elements"].as_array().unwrap();
        // Should have a div (prompt) and an action with buttons (3 options)
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["tag"].as_str(), Some("div"));

        let buttons = elements[1]["actions"].as_array().unwrap();
        assert_eq!(buttons.len(), 3);
        // Each button should have select action with answer value
        assert_eq!(buttons[0]["value"]["action"].as_str(), Some("select"));
        assert_eq!(buttons[0]["value"]["answer"].as_str(), Some("Option A"));
        assert_eq!(buttons[0]["value"]["request_id"].as_str(), Some("req-ss-1"));
        assert_eq!(buttons[1]["value"]["answer"].as_str(), Some("Option B"));
        assert_eq!(buttons[2]["value"]["answer"].as_str(), Some("Option C"));
    }

    #[test]
    fn test_build_single_select_card_many_options() {
        let platform = test_platform();
        let options: Vec<String> = (1..=8)
            .map(|i| format!("Option {}", i))
            .collect();
        let card = platform.build_single_select_card("req-ss-2", "Pick one", &options);
        assert_eq!(card["schema"].as_str(), Some("2.0"));

        let elements = card["body"]["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        // Many options (>5) uses select_static, not buttons
        let action_wrapper = &elements[1];
        let actions = action_wrapper["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["tag"].as_str(), Some("select_static"));
        // The select_static should have options
        let select_options = actions[0]["options"].as_array().unwrap();
        assert_eq!(select_options.len(), 8);
        assert_eq!(select_options[0]["value"].as_str(), Some("Option 1"));
    }

    #[test]
    fn test_build_single_select_card_prompt_truncation() {
        let platform = test_platform();
        let options: Vec<String> = vec!["A".into(), "B".into()];
        let long_prompt = format!("{} and more", "x".repeat(100));
        let card = platform.build_single_select_card("req-ss-3", &long_prompt, &options);

        let elements = card["body"]["elements"].as_array().unwrap();
        let div = &elements[0];
        let prompt_text = div["text"]["content"].as_str().unwrap();
        assert!(prompt_text.ends_with("..."));
        assert!(!prompt_text.contains("and more"));
    }

    // ------------------------------------------------------------------
    // build_multi_select_card (and displayer)
    // ------------------------------------------------------------------

    #[test]
    fn test_build_multi_select_displayer() {
        let platform = test_platform();
        let options: Vec<String> = vec!["Alpha".into(), "Beta".into(), "Gamma".into()];
        let selected: Vec<String> = vec!["Alpha".into()];
        let card = platform.build_multi_select_card(
            "req-ms-1",
            "Choose several",
            &options,
            &selected,
        );
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        assert_eq!(card["header"]["template"].as_str(), Some("blue"));

        let elements = card["body"]["elements"].as_array().unwrap();
        // Should have toggles action + submit/cancel action
        assert_eq!(elements.len(), 2);

        // First action: toggle buttons
        let toggle_actions = elements[0]["actions"].as_array().unwrap();
        assert_eq!(toggle_actions.len(), 3);

        // Alpha should be marked as selected (✅ prefix)
        assert_eq!(toggle_actions[0]["value"]["action"].as_str(), Some("toggle_select"));
        assert_eq!(toggle_actions[0]["value"]["toggle"].as_str(), Some("Alpha"));
        let alpha_label = toggle_actions[0]["text"]["content"].as_str().unwrap();
        assert!(alpha_label.starts_with("✅"));

        // Beta and Gamma should NOT be selected
        let beta_label = toggle_actions[1]["text"]["content"].as_str().unwrap();
        assert!(!beta_label.starts_with("✅"));
        assert_eq!(beta_label, "Beta");

        // Submit and cancel buttons
        let control_actions = elements[1]["actions"].as_array().unwrap();
        assert_eq!(control_actions.len(), 2);
        assert_eq!(control_actions[0]["value"]["action"].as_str(), Some("submit_multi"));
        assert_eq!(control_actions[1]["value"]["action"].as_str(), Some("cancel_multi"));
    }

    #[test]
    fn test_build_multi_select_prompt_truncation() {
        let platform = test_platform();
        let options: Vec<String> = vec!["A".into(), "B".into()];
        let selected: Vec<String> = vec![];
        let long_prompt = format!("{} and more", "x".repeat(100));
        let card = platform.build_multi_select_card(
            "req-ms-2",
            &long_prompt,
            &options,
            &selected,
        );
        let title = card["header"]["title"]["content"].as_str().unwrap();
        assert!(title.ends_with("..."));
    }

    // ------------------------------------------------------------------
    // build_text_input_hint_card
    // ------------------------------------------------------------------

    #[test]
    fn test_build_text_input_hint_card() {
        let platform = test_platform();
        let card = platform.build_text_input_hint_card("req-ti-1", "What is your name?");
        assert_eq!(card["schema"].as_str(), Some("2.0"));
        assert_eq!(card["header"]["template"].as_str(), Some("wathet"));
        assert_eq!(
            card["header"]["title"]["content"].as_str(),
            Some("What is your name?")
        );

        let elements = card["body"]["elements"].as_array().unwrap();
        // Should have a div with instruction + an action with a cancel button
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["tag"].as_str(), Some("div"));
        // Instruction text
        assert!(elements[0]["text"]["content"]
            .as_str()
            .unwrap()
            .contains("直接回复"));

        let action = &elements[1];
        let buttons = action["actions"].as_array().unwrap();
        assert_eq!(buttons.len(), 1);
        assert_eq!(
            buttons[0]["value"]["action"].as_str(),
            Some("cancel_text_input")
        );
        assert_eq!(
            buttons[0]["value"]["request_id"].as_str(),
            Some("req-ti-1")
        );
    }

    #[test]
    fn test_build_text_input_hint_card_prompt_truncation() {
        let platform = test_platform();
        let long_prompt = format!("{} and more", "x".repeat(100));
        let card = platform.build_text_input_hint_card("req-ti-2", &long_prompt);
        let title = card["header"]["title"]["content"].as_str().unwrap();
        assert!(title.ends_with("..."));
        assert!(!title.contains("and more"));
    }
}
