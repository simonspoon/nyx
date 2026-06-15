use serde::Deserialize;

/// A single line from a JSONL conversation file.
/// Fields may appear unused but are required for serde deserialization.
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
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
        #[serde(rename = "requestId")]
        request_id: Option<String>,
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
    ToolResult {
        content: Option<ToolResultContent>,
        #[serde(rename = "tool_use_id")]
        tool_use_id: Option<String>,
        is_error: Option<bool>,
    },
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
    pub usage: Option<Usage>,
}

/// Token usage and cost-relevant metadata from an assistant message.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub service_tier: Option<String>,
    pub speed: Option<String>,
    pub cache_creation: Option<CacheCreation>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CacheCreation {
    pub ephemeral_5m_input_tokens: Option<i64>,
    pub ephemeral_1h_input_tokens: Option<i64>,
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
    ToolUse {
        id: Option<String>,
        name: Option<String>,
        input: Option<serde_json::Value>,
    },
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

    /// Get the token usage (only on assistant records that carry it).
    #[allow(dead_code)]
    pub fn usage(&self) -> Option<&Usage> {
        match self {
            Record::Assistant { message, .. } => message.usage.as_ref(),
            _ => None,
        }
    }

    /// Get the model name (only on assistant records).
    #[allow(dead_code)]
    pub fn model(&self) -> Option<&str> {
        match self {
            Record::Assistant { message, .. } => message.model.as_deref(),
            _ => None,
        }
    }

    /// Get the request ID (only on assistant records).
    #[allow(dead_code)]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Record::Assistant { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }

    /// Get the tool_use blocks (id, name, input) from an assistant record.
    #[allow(dead_code)]
    pub fn tool_uses(&self) -> Vec<(Option<&str>, Option<&str>, Option<&serde_json::Value>)> {
        let mut uses = Vec::new();
        if let Record::Assistant {
            message:
                AssistantMessage {
                    content: Some(AssistantContent::Blocks(blocks)),
                    ..
                },
            ..
        } = self
        {
            for block in blocks {
                if let AssistantContentBlock::ToolUse { id, name, input } = block {
                    uses.push((id.as_deref(), name.as_deref(), input.as_ref()));
                }
            }
        }
        uses
    }

    /// Get the tool_result blocks (tool_use_id, is_error) from a user record.
    #[allow(dead_code)]
    pub fn tool_results(&self) -> Vec<(Option<&str>, Option<bool>)> {
        let mut results = Vec::new();
        if let Record::User {
            message:
                UserMessage {
                    content: UserContent::Blocks(blocks),
                    ..
                },
            ..
        } = self
        {
            for block in blocks {
                if let UserContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } = block
                {
                    results.push((tool_use_id.as_deref(), *is_error));
                }
            }
        }
        results
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
                        UserContentBlock::ToolResult {
                            content: Some(c), ..
                        } => {
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

    #[test]
    fn parse_assistant_usage_and_tool_use() {
        // Real-shape assistant line: usage object (with cache_creation split),
        // model, requestId, and a tool_use block.
        let json = r#"{
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "reading file"},
                    {"type": "tool_use", "id": "toolu_01EF", "name": "Read",
                     "input": {"file_path": "/tmp/x"}, "caller": {"type": "direct"}}
                ],
                "model": "claude-opus-4-8",
                "usage": {
                    "input_tokens": 2832,
                    "cache_creation_input_tokens": 23317,
                    "cache_read_input_tokens": 0,
                    "output_tokens": 1160,
                    "service_tier": "standard",
                    "cache_creation": {
                        "ephemeral_1h_input_tokens": 23317,
                        "ephemeral_5m_input_tokens": 0
                    },
                    "speed": "standard"
                }
            },
            "timestamp": "2026-06-14T01:00:00Z",
            "sessionId": "abc-123",
            "slug": "a-slug",
            "requestId": "req_011Cc2"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.model(), Some("claude-opus-4-8"));
        assert_eq!(record.request_id(), Some("req_011Cc2"));

        let usage = record.usage().unwrap();
        assert_eq!(usage.input_tokens, Some(2832));
        assert_eq!(usage.output_tokens, Some(1160));
        assert_eq!(usage.cache_creation_input_tokens, Some(23317));
        assert_eq!(usage.cache_read_input_tokens, Some(0));
        assert_eq!(usage.service_tier.as_deref(), Some("standard"));
        assert_eq!(usage.speed.as_deref(), Some("standard"));
        let cc = usage.cache_creation.as_ref().unwrap();
        assert_eq!(cc.ephemeral_5m_input_tokens, Some(0));
        assert_eq!(cc.ephemeral_1h_input_tokens, Some(23317));

        let uses = record.tool_uses();
        assert_eq!(uses.len(), 1);
        let (id, name, input) = uses[0];
        assert_eq!(id, Some("toolu_01EF"));
        assert_eq!(name, Some("Read"));
        assert_eq!(input.unwrap()["file_path"], "/tmp/x");
    }

    #[test]
    fn parse_user_tool_result_is_error() {
        // Real-shape user line: tool_result with tool_use_id and is_error.
        let json = r#"{
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_01EF",
                 "content": "command failed", "is_error": true}
            ]},
            "timestamp": "2026-06-14T01:00:01Z",
            "sessionId": "abc-123"
        }"#;
        let record: Record = serde_json::from_str(json).unwrap();
        let results = record.tool_results();
        assert_eq!(results.len(), 1);
        let (tool_use_id, is_error) = results[0];
        assert_eq!(tool_use_id, Some("toolu_01EF"));
        assert_eq!(is_error, Some(true));
        // text extraction still works alongside the new fields.
        assert_eq!(record.extract_text().unwrap(), "command failed");
    }
}
