use serde_json::Value;

/// Standardize transcript to MemPalace format:
/// ```text
/// > user turn
/// ai response
///
/// > next user turn
/// next ai response
/// ```
pub fn normalize_transcript(content: &str) -> String {
    if content.trim().is_empty() {
        return content.to_string();
    }

    // Already has > markers — pass through
    let lines: Vec<&str> = content.lines().collect();
    let quote_count = lines.iter().filter(|l| l.trim().starts_with('>')).count();
    if quote_count >= 3 {
        return content.to_string();
    }

    // Try JSON normalization
    let trimmed = content.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Some(normalized) = try_normalize_json(content) {
            return normalized;
        }
    }

    content.to_string()
}

fn try_normalize_json(content: &str) -> Option<String> {
    // Claude Code JSONL
    if let Some(normalized) = try_claude_code_jsonl(content) {
        return Some(normalized);
    }

    let data: Value = serde_json::from_str(content).ok()?;

    if let Some(normalized) = try_claude_ai_json(&data) {
        return Some(normalized);
    }
    if let Some(normalized) = try_chatgpt_json(&data) {
        return Some(normalized);
    }
    if let Some(normalized) = try_slack_json(&data) {
        return Some(normalized);
    }

    None
}

fn try_claude_code_jsonl(content: &str) -> Option<String> {
    let mut messages = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(line).ok()?;
        let msg_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let message = entry.get("message");
        if msg_type == "human" {
            if let Some(m) = message {
                let text = extract_content(m.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    messages.push(("user".to_string(), text));
                }
            }
        } else if msg_type == "assistant" {
            if let Some(m) = message {
                let text = extract_content(m.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    messages.push(("assistant".to_string(), text));
                }
            }
        }
    }
    if messages.len() >= 2 {
        return Some(messages_to_transcript(messages));
    }
    None
}

fn try_claude_ai_json(data: &Value) -> Option<String> {
    let list = if data.is_array() {
        Some(data.as_array().unwrap())
    } else if let Some(obj) = data.as_object() {
        obj.get("messages")
            .and_then(|v| v.as_array())
            .or_else(|| obj.get("chat_messages").and_then(|v| v.as_array()))
    } else {
        None
    };

    let list = list?;
    let mut messages = Vec::new();
    for item in list {
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let text = extract_content(item.get("content").unwrap_or(&Value::Null));
        if (role == "user" || role == "human") && !text.is_empty() {
            messages.push(("user".to_string(), text));
        } else if (role == "assistant" || role == "ai") && !text.is_empty() {
            messages.push(("assistant".to_string(), text));
        }
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(messages));
    }
    None
}

fn try_chatgpt_json(data: &Value) -> Option<String> {
    let mapping = data.get("mapping").and_then(|v| v.as_object())?;
    let mut messages = Vec::new();

    let mut root_id = None;
    let mut fallback_root = None;

    for (node_id, node) in mapping {
        if node.get("parent").is_none() || node.get("parent").unwrap().is_null() {
            if node.get("message").is_none() || node.get("message").unwrap().is_null() {
                root_id = Some(node_id.clone());
                break;
            } else if fallback_root.is_none() {
                fallback_root = Some(node_id.clone());
            }
        }
    }

    let root_id = root_id.or(fallback_root)?;

    let mut current_id = Some(root_id);
    let mut visited = std::collections::HashSet::new();

    while let Some(id) = current_id {
        if !visited.insert(id.clone()) {
            break;
        }

        let node = mapping.get(&id)?;
        if let Some(msg) = node.get("message") {
            let role = msg
                .get("author")
                .and_then(|v| v.get("role"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = msg.get("content");
            let mut text = String::new();
            if let Some(c) = content {
                if let Some(parts) = c.get("parts").and_then(|v| v.as_array()) {
                    for p in parts {
                        if let Some(s) = p.as_str() {
                            if !text.is_empty() {
                                text.push(' ');
                            }
                            text.push_str(s);
                        }
                    }
                }
            }
            let text = text.trim();
            if role == "user" && !text.is_empty() {
                messages.push(("user".to_string(), text.to_string()));
            } else if role == "assistant" && !text.is_empty() {
                messages.push(("assistant".to_string(), text.to_string()));
            }
        }

        current_id = node
            .get("children")
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(messages));
    }
    None
}

fn try_slack_json(data: &Value) -> Option<String> {
    let list = data.as_array()?;
    let mut messages = Vec::new();
    let mut seen_users = std::collections::HashMap::new();
    let mut last_role = None;

    for item in list {
        if item.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let user_id = item
            .get("user")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("username").and_then(|v| v.as_str()))
            .unwrap_or("");
        let text = item
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if user_id.is_empty() || text.is_empty() {
            continue;
        }

        let role = if let Some(role) = seen_users.get(user_id) {
            *role
        } else {
            let role = if seen_users.is_empty() {
                "user"
            } else if last_role == Some("user") {
                "assistant"
            } else {
                "user"
            };
            seen_users.insert(user_id.to_string(), role);
            role
        };
        last_role = Some(role);
        messages.push((role.to_string(), text.to_string()));
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(messages));
    }
    None
}

fn extract_content(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.trim().to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                parts.push(s.to_string());
            } else if let Some(obj) = item.as_object() {
                if obj.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
        return parts.join(" ").trim().to_string();
    }
    if let Some(obj) = content.as_object() {
        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
            return text.trim().to_string();
        }
    }
    "".to_string()
}

fn messages_to_transcript(messages: Vec<(String, String)>) -> String {
    let mut lines = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let (role, text) = &messages[i];
        if role == "user" {
            lines.push(format!("> {}", text));
            if i + 1 < messages.len() && messages[i + 1].0 == "assistant" {
                lines.push(messages[i + 1].1.clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            lines.push(text.clone());
            i += 1;
        }
        lines.push("".to_string());
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_plain_text() {
        let content = "Hello world\nThis is a test";
        assert_eq!(normalize_transcript(content), content);
    }

    #[test]
    fn test_normalize_existing_transcript() {
        let content = "> hello\nworld\n\n> how are you?\nfine";
        // It has 2 > markers, and we check for >= 3 in the code.
        // Wait, the python code said: if sum(1 for line in lines if line.strip().startswith(">")) >= 3: return content
        // So 2 markers might be normalized?
        // Actually, if it's plain text and not JSON, it returns content anyway.
        assert_eq!(normalize_transcript(content), content);
    }

    #[test]
    fn test_normalize_claude_code() {
        let jsonl = r#"{"type": "human", "message": {"content": "hello"}}
{"type": "assistant", "message": {"content": "hi there"}}"#;
        let expected = "> hello\nhi there";
        assert_eq!(normalize_transcript(jsonl), expected);
    }

    #[test]
    fn test_normalize_claude_ai() {
        let json = r#"[
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"}
        ]"#;
        let expected = "> hello\nhi";
        assert_eq!(normalize_transcript(json), expected);
    }

    #[test]
    fn test_normalize_chatgpt() {
        let json = r#"{
            "mapping": {
                "root": {
                    "parent": null,
                    "message": null,
                    "children": ["msg1"]
                },
                "msg1": {
                    "parent": "root",
                    "message": {
                        "author": {"role": "user"},
                        "content": {"parts": ["hello"]}
                    },
                    "children": ["msg2"]
                },
                "msg2": {
                    "parent": "msg1",
                    "message": {
                        "author": {"role": "assistant"},
                        "content": {"parts": ["hi"]}
                    },
                    "children": []
                }
            }
        }"#;
        let expected = "> hello\nhi";
        assert_eq!(normalize_transcript(json), expected);
    }

    #[test]
    fn test_normalize_slack() {
        let json = r#"[
            {"type": "message", "user": "U1", "text": "hello"},
            {"type": "message", "user": "U2", "text": "hi"}
        ]"#;
        let expected = "> hello\nhi";
        assert_eq!(normalize_transcript(json), expected);
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_transcript("   "), "   ");
    }

    #[test]
    fn test_normalize_passthrough() {
        let content = "> 1\n2\n\n> 3\n4\n\n> 5\n6";
        assert_eq!(normalize_transcript(content), content);
    }

    #[test]
    fn test_normalize_invalid_json() {
        let content = "{ \"invalid\": ";
        assert_eq!(normalize_transcript(content), content);
    }

    #[test]
    fn test_normalize_claude_ai_chat_messages() {
        let json = r#"{
            "chat_messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "hi"}
            ]
        }"#;
        let expected = "> hello\nhi";
        assert_eq!(normalize_transcript(json), expected);
    }

    #[test]
    fn test_normalize_chatgpt_fallback() {
        // Root has a message instead of being null
        let json = r#"{
            "mapping": {
                "root": {
                    "parent": null,
                    "message": {
                        "author": {"role": "user"},
                        "content": {"parts": ["root msg"]}
                    },
                    "children": ["msg2"]
                },
                "msg2": {
                    "parent": "root",
                    "message": {
                        "author": {"role": "assistant"},
                        "content": {"parts": ["hi"]}
                    },
                    "children": []
                }
            }
        }"#;
        let expected = "> root msg\nhi";
        assert_eq!(normalize_transcript(json), expected);
    }

    #[test]
    fn test_normalize_slack_multiple_users() {
        let json = r#"[
            {"type": "message", "user": "U1", "text": "msg1"},
            {"type": "message", "user": "U2", "text": "msg2"},
            {"type": "message", "user": "U3", "text": "msg3"}
        ]"#;
        let res = normalize_transcript(json);
        assert!(res.contains("> msg1"));
        assert!(res.contains("msg2"));
        // U3 should be assigned "user" role because U2 was "assistant"
        assert!(res.contains("> msg3"));
    }

    #[test]
    fn test_extract_content_complex() {
        let content = json!([
            {"type": "text", "text": "part1"},
            "part2",
            {"other": "ignore"}
        ]);
        assert_eq!(extract_content(&content), "part1 part2");

        let content_obj = json!({"text": "only text"});
        assert_eq!(extract_content(&content_obj), "only text");
    }

    #[test]
    fn test_messages_to_transcript_adjacent() {
        let messages = vec![
            ("assistant".to_string(), "hi".to_string()),
            ("assistant".to_string(), "there".to_string()),
        ];
        let res = messages_to_transcript(messages);
        assert_eq!(res, "hi\n\nthere");
    }
}
