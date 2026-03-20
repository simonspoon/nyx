use serde::Deserialize;

/// A single line from a JSONL conversation file.
/// Fields may appear unused but are required for serde deserialization.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Record {
    #[serde(rename = "user")]
    User {
        message: UserMessage,
        timestamp: Option<String>,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
    },

    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        timestamp: Option<String>,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        slug: Option<String>,
    },

    #[serde(rename = "system")]
    System {
        subtype: Option<String>,
        timestamp: Option<String>,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        slug: Option<String>,
    },

    #[serde(rename = "custom-title")]
    CustomTitle {
        #[serde(rename = "customTitle")]
        custom_title: Option<String>,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
    },

    #[serde(rename = "progress")]
    Progress {},

    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {},
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct UserMessage {
    pub role: Option<String>,
    pub content: UserContent,
}

/// User message content can be either a plain string or an array of blocks.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<UserContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum UserContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_result")]
    ToolResult { content: Option<ToolResultContent> },
    #[serde(other)]
    Other,
}

/// Tool result content can be a string or structured.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ToolResultBlock {
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    pub text: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    pub role: Option<String>,
    pub content: Option<AssistantContent>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AssistantContent {
    Text(String),
    Blocks(Vec<AssistantContentBlock>),
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: Option<String> },
    #[serde(rename = "tool_use")]
    ToolUse {},
    #[serde(rename = "tool_result")]
    ToolResult {},
    #[serde(other)]
    Other,
}

impl Record {
    /// Extract displayable text content from a record.
    /// Returns None for records that don't contain useful text (progress, file-history-snapshot).
    pub fn extract_text(&self) -> Option<String> {
        match self {
            Record::User { message, .. } => Some(message.extract_text()),
            Record::Assistant { message, .. } => message.extract_text(),
            Record::System { subtype, .. } => subtype.as_ref().map(|s| format!("[system: {}]", s)),
            Record::CustomTitle { custom_title, .. } => {
                custom_title.as_ref().map(|t| format!("[title: {}]", t))
            }
            Record::Progress {} | Record::FileHistorySnapshot {} => None,
        }
    }

    /// Get the role string for display purposes.
    pub fn role(&self) -> Option<&str> {
        match self {
            Record::User { .. } => Some("user"),
            Record::Assistant { .. } => Some("assistant"),
            Record::System { .. } => Some("system"),
            Record::CustomTitle { .. } => Some("system"),
            Record::Progress {} | Record::FileHistorySnapshot {} => None,
        }
    }

    /// Get the timestamp string.
    pub fn timestamp(&self) -> Option<&str> {
        match self {
            Record::User { timestamp, .. } => timestamp.as_deref(),
            Record::Assistant { timestamp, .. } => timestamp.as_deref(),
            Record::System { timestamp, .. } => timestamp.as_deref(),
            _ => None,
        }
    }

    /// Get the session ID.
    #[allow(dead_code)]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Record::User { session_id, .. } => session_id.as_deref(),
            Record::Assistant { session_id, .. } => session_id.as_deref(),
            Record::System { session_id, .. } => session_id.as_deref(),
            Record::CustomTitle { session_id, .. } => session_id.as_deref(),
            _ => None,
        }
    }

    /// Get the slug (only on assistant and system records).
    pub fn slug(&self) -> Option<&str> {
        match self {
            Record::Assistant { slug, .. } => slug.as_deref(),
            Record::System { slug, .. } => slug.as_deref(),
            _ => None,
        }
    }
}

impl UserMessage {
    pub fn extract_text(&self) -> String {
        match &self.content {
            UserContent::Text(s) => s.clone(),
            UserContent::Blocks(blocks) => {
                let mut parts = Vec::new();
                for block in blocks {
                    match block {
                        UserContentBlock::Text { text } => parts.push(text.clone()),
                        UserContentBlock::ToolResult { content: Some(c) } => {
                            if let Some(text) = c.extract_text() {
                                parts.push(text);
                            }
                        }
                        _ => {}
                    }
                }
                parts.join("\n")
            }
        }
    }
}

impl ToolResultContent {
    pub fn extract_text(&self) -> Option<String> {
        match self {
            ToolResultContent::Text(s) => Some(s.clone()),
            ToolResultContent::Blocks(blocks) => {
                let texts: Vec<String> = blocks.iter().filter_map(|b| b.text.clone()).collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join("\n"))
                }
            }
        }
    }
}

impl AssistantMessage {
    pub fn extract_text(&self) -> Option<String> {
        match &self.content {
            None => None,
            Some(AssistantContent::Text(s)) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            }
            Some(AssistantContent::Blocks(blocks)) => {
                let mut parts = Vec::new();
                for block in blocks {
                    if let AssistantContentBlock::Text { text } = block
                        && !text.is_empty()
                    {
                        parts.push(text.clone());
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_string_content() {
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": "hello world"},
            "timestamp": "2026-03-20T01:00:00Z",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.extract_text().unwrap(), "hello world");
        assert_eq!(record.role(), Some("user"));
        assert_eq!(record.session_id(), Some("abc-123"));
    }

    #[test]
    fn parse_user_array_content() {
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "text", "text": "first part"},
                {"type": "tool_result", "content": "tool output"}
            ]},
            "timestamp": "2026-03-20T01:00:00Z",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert!(text.contains("first part"));
        assert!(text.contains("tool output"));
    }

    #[test]
    fn parse_assistant_text_blocks() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "let me think"},
                    {"type": "text", "text": "Here is my response"},
                    {"type": "tool_use", "id": "t1", "name": "Read", "input": {}}
                ],
                "model": "claude-opus-4-6"
            },
            "timestamp": "2026-03-20T01:00:00Z",
            "sessionId": "abc-123",
            "slug": "luminous-toasting-ember"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.extract_text().unwrap(), "Here is my response");
        assert_eq!(record.slug(), Some("luminous-toasting-ember"));
    }

    #[test]
    fn parse_system_record() {
        let json = r#"{
            "type": "system",
            "subtype": "stop_hook_summary",
            "timestamp": "2026-03-20T01:00:00Z",
            "sessionId": "abc-123",
            "slug": "luminous-toasting-ember"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.role(), Some("system"));
        assert!(record.extract_text().unwrap().contains("stop_hook_summary"));
    }

    #[test]
    fn parse_progress_record() {
        let json = r#"{
            "type": "progress",
            "data": {"type": "hook_progress"},
            "toolUseID": "abc"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
        assert!(record.role().is_none());
    }

    #[test]
    fn parse_file_history_snapshot() {
        let json = r#"{
            "type": "file-history-snapshot",
            "messageId": "abc",
            "snapshot": {"trackedFileBackups": {}}
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }

    #[test]
    fn parse_custom_title() {
        let json = r#"{
            "type": "custom-title",
            "customTitle": "My Chat",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert!(text.contains("My Chat"));
    }

    #[test]
    fn parse_real_user_tool_result_array() {
        // Real-world: user content is array with tool_result blocks containing string content
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_abc", "content": "file contents here"}
            ]},
            "timestamp": "2026-03-20T01:00:00Z",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert!(text.contains("file contents here"));
    }

    #[test]
    fn custom_title_role_is_system() {
        let json = r#"{
            "type": "custom-title",
            "customTitle": "My Chat",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.role(), Some("system"));
    }

    #[test]
    fn session_id_extraction() {
        // User record
        let json = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-01-01T00:00:00Z","sessionId":"user-sess"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.session_id(), Some("user-sess"));

        // Assistant record
        let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"model":"m"},"timestamp":"2026-01-01T00:00:00Z","sessionId":"asst-sess","slug":"s"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.session_id(), Some("asst-sess"));

        // System record
        let json = r#"{"type":"system","subtype":"stop","timestamp":"2026-01-01T00:00:00Z","sessionId":"sys-sess","slug":"s"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.session_id(), Some("sys-sess"));

        // CustomTitle record
        let json = r#"{"type":"custom-title","customTitle":"Title","sessionId":"ct-sess"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.session_id(), Some("ct-sess"));

        // Progress record has no session_id
        let json = r#"{"type":"progress","data":{"type":"hook_progress"},"toolUseID":"abc"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.session_id(), None);
    }

    #[test]
    fn system_record_slug() {
        let json = r#"{"type":"system","subtype":"stop","timestamp":"2026-01-01T00:00:00Z","sessionId":"s1","slug":"my-slug"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.slug(), Some("my-slug"));
    }

    #[test]
    fn user_slug_returns_none() {
        let json = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-01-01T00:00:00Z","sessionId":"s1"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.slug(), None);
    }

    #[test]
    fn tool_result_with_none_content() {
        // User message with tool_result that has no content
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1"},
                {"type": "text", "text": "follow up"}
            ]},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert_eq!(text, "follow up");
    }

    #[test]
    fn tool_result_content_blocks() {
        // Tool result with block-based content
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": [
                    {"type": "text", "text": "block one"},
                    {"type": "text", "text": "block two"}
                ]}
            ]},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert!(text.contains("block one"));
        assert!(text.contains("block two"));
    }

    #[test]
    fn tool_result_content_blocks_empty() {
        // Tool result blocks with no text fields
        let content = ToolResultContent::Blocks(vec![ToolResultBlock {
            block_type: Some("image".to_string()),
            text: None,
        }]);
        assert!(content.extract_text().is_none());
    }

    #[test]
    fn assistant_empty_string_content() {
        let json = r#"{
            "type": "assistant",
            "message": {"role": "assistant", "content": "", "model": "m"},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }

    #[test]
    fn assistant_no_content() {
        let json = r#"{
            "type": "assistant",
            "message": {"role": "assistant", "model": "m"},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }

    #[test]
    fn assistant_blocks_only_tool_use() {
        // Assistant message with only tool_use blocks (no text) => None
        let json = r#"{
            "type": "assistant",
            "message": {"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Read", "input": {}}
            ], "model": "m"},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }

    #[test]
    fn assistant_blocks_with_empty_text() {
        // Assistant message with empty text block => None
        let json = r#"{
            "type": "assistant",
            "message": {"role": "assistant", "content": [
                {"type": "text", "text": ""}
            ], "model": "m"},
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionId": "s1"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }

    #[test]
    fn custom_title_extract_text() {
        // CustomTitle with title
        let json = r#"{"type":"custom-title","customTitle":"My Title","sessionId":"s1"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let text = record.extract_text().unwrap();
        assert!(text.contains("My Title"));

        // CustomTitle with null title
        let json = r#"{"type":"custom-title","sessionId":"s1"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert!(record.extract_text().is_none());
    }
}
