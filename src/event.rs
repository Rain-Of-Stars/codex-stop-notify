// 事件模块：解析兼容 Hook stdin 或 Codex notify 参数
use crate::transcript::Turn;
use serde::Deserialize;

/// stdin 大小上限（10 MB）
const MAX_STDIN_SIZE: usize = 10 * 1024 * 1024;

/// 通知来源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Codex,
    LegacyHook,
}

/// Hook 输入结构体（历史兼容模式从 stdin 传入的 JSON）
/// 注意：旧 Hook 实际常见 snake_case 字段名（hook_event_name, session_id），
/// 同时保留 camelCase 别名以兼容不同来源的格式变更
#[derive(Debug, Deserialize)]
pub struct HookInput {
    /// 事件时间戳
    pub timestamp: Option<String>,
    /// 工作目录（不可信，不用于安全判断）
    pub cwd: Option<String>,
    /// 会话唯一标识
    #[serde(alias = "sessionId")]
    pub session_id: Option<String>,
    /// 事件名称（Stop / SubagentStop / ...）
    #[serde(alias = "hookEventName")]
    pub hook_event_name: String,
    /// transcript 文件路径
    #[serde(alias = "transcriptPath")]
    pub transcript_path: Option<String>,
    /// 是否因前一个 Stop hook 触发的继续运行（防止无限循环）
    #[serde(alias = "stopHookActive")]
    pub stop_hook_active: Option<bool>,
}

/// Codex notify 事件输入
#[derive(Debug, Deserialize)]
pub struct CodexNotifyInput {
    /// 事件名称（当前仅支持 agent-turn-complete）
    #[serde(rename = "type")]
    pub event_type: String,
    /// 线程标识
    #[serde(rename = "thread-id", alias = "thread_id", alias = "threadId")]
    pub thread_id: Option<String>,
    /// 轮次标识
    #[serde(rename = "turn-id", alias = "turn_id", alias = "turnId")]
    pub turn_id: Option<String>,
    /// 当前工作目录
    pub cwd: Option<String>,
    /// 触发本轮的用户消息列表
    #[serde(
        rename = "input-messages",
        alias = "input_messages",
        alias = "inputMessages",
        default
    )]
    pub input_messages: Vec<String>,
    /// 最后一条助手消息
    #[serde(
        rename = "last-assistant-message",
        alias = "last_assistant_message",
        alias = "lastAssistantMessage"
    )]
    pub last_assistant_message: Option<String>,
}

/// 从 stdin 读取并解析 Hook 输入
pub fn read_hook_input() -> Result<HookInput, String> {
    use std::io::Read;
    let mut buffer = Vec::new();
    let bytes_read = std::io::stdin()
        .take(MAX_STDIN_SIZE as u64)
        .read_to_end(&mut buffer)
        .map_err(|e| format!("读取 stdin 失败: {}", e))?;

    if bytes_read == 0 {
        return Err("stdin 为空，没有接收到 Hook 输入".to_string());
    }

    if bytes_read >= MAX_STDIN_SIZE {
        return Err(format!("stdin 超过大小限制 ({} bytes)", MAX_STDIN_SIZE));
    }

    serde_json::from_slice(&buffer).map_err(|e| format!("解析 Hook 输入 JSON 失败: {}", e))
}

/// 解析 Codex notify 事件 JSON 参数
pub fn parse_codex_notify_input(payload: &str) -> Result<CodexNotifyInput, String> {
    serde_json::from_str(payload).map_err(|e| format!("解析 Codex notify JSON 失败: {}", e))
}

/// 判断是否应该处理兼容 Hook 事件
/// 仅处理 Stop 事件，忽略 SubagentStop 和其他事件
/// 当 stop_hook_active=true 时跳过（防止无限循环）
pub fn should_process(input: &HookInput) -> Result<bool, String> {
    if input.hook_event_name != "Stop" {
        return Ok(false);
    }

    if input.stop_hook_active.unwrap_or(false) {
        return Ok(false);
    }

    if input.transcript_path.is_none() {
        return Err("Stop 事件缺少 transcript_path".to_string());
    }

    Ok(true)
}

/// 判断是否应处理 Codex notify 事件
pub fn should_process_codex(input: &CodexNotifyInput) -> bool {
    input.event_type == "agent-turn-complete"
}

/// 将 Codex notify 事件转为邮件展示所需的轮次列表
pub fn build_codex_turns(input: &CodexNotifyInput) -> Vec<Turn> {
    let mut turns = Vec::new();

    for message in &input.input_messages {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            turns.push(Turn {
                role: "user".to_string(),
                content: trimmed.to_string(),
            });
        }
    }

    let assistant_content = input
        .last_assistant_message
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("任务已完成，Codex 未附带 last-assistant-message。")
        .to_string();

    turns.push(Turn {
        role: "assistant".to_string(),
        content: assistant_content,
    });

    turns
}

/// 构建 Codex 事件的幂等键
pub fn build_codex_dedup_key(input: &CodexNotifyInput) -> Option<String> {
    let thread_id = input
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let turn_id = input
        .turn_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (thread_id, turn_id) {
        (Some(thread), Some(turn)) => Some(format!("codex:{}:{}", thread, turn)),
        (Some(thread), None) => Some(format!("codex:{}", thread)),
        (None, Some(turn)) => Some(format!("codex-turn:{}", turn)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(event: &str, stop_active: Option<bool>, transcript: Option<&str>) -> HookInput {
        HookInput {
            timestamp: Some("2026-04-03T10:00:00.000Z".to_string()),
            cwd: Some("/workspace".to_string()),
            session_id: Some("test-session-123".to_string()),
            hook_event_name: event.to_string(),
            transcript_path: transcript.map(|s| s.to_string()),
            stop_hook_active: stop_active,
        }
    }

    #[test]
    fn test_should_process_stop_event() {
        let input = make_input("Stop", None, Some("/path/to/transcript.json"));
        assert!(should_process(&input).unwrap());
    }

    #[test]
    fn test_should_skip_subagent_stop() {
        let input = make_input("SubagentStop", None, Some("/path/to/transcript.json"));
        assert!(!should_process(&input).unwrap());
    }

    #[test]
    fn test_should_skip_other_events() {
        for event in &[
            "SessionStart",
            "PreToolUse",
            "PostToolUse",
            "UserPromptSubmit",
        ] {
            let input = make_input(event, None, Some("/path"));
            assert!(!should_process(&input).unwrap());
        }
    }

    #[test]
    fn test_should_skip_when_stop_hook_active() {
        let input = make_input("Stop", Some(true), Some("/path/to/transcript.json"));
        assert!(!should_process(&input).unwrap());
    }

    #[test]
    fn test_should_error_without_transcript_path() {
        let input = make_input("Stop", None, None);
        assert!(should_process(&input).is_err());
    }

    #[test]
    fn test_deserialize_hook_input() {
        let json = r#"{
            "timestamp": "2026-04-03T10:00:00.000Z",
            "cwd": "/workspace",
            "sessionId": "abc-123",
            "hookEventName": "Stop",
            "transcript_path": "/tmp/transcript.json",
            "stop_hook_active": false
        }"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "Stop");
        assert_eq!(input.session_id.as_deref(), Some("abc-123"));
        assert_eq!(
            input.transcript_path.as_deref(),
            Some("/tmp/transcript.json")
        );
        assert_eq!(input.stop_hook_active, Some(false));
    }

    #[test]
    fn test_deserialize_minimal_input() {
        let json = r#"{"hookEventName": "Stop"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "Stop");
        assert!(input.session_id.is_none());
        assert!(input.transcript_path.is_none());
    }

    #[test]
    fn test_deserialize_snake_case_input() {
        let json = r#"{
            "timestamp": "2026-04-03T10:00:00.000Z",
            "session_id": "snake-test-001",
            "hook_event_name": "Stop",
            "transcript_path": "/tmp/transcript.json",
            "stop_hook_active": false
        }"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "Stop");
        assert_eq!(input.session_id.as_deref(), Some("snake-test-001"));
        assert_eq!(
            input.transcript_path.as_deref(),
            Some("/tmp/transcript.json")
        );
        assert_eq!(input.stop_hook_active, Some(false));
    }

    #[test]
    fn test_deserialize_minimal_snake_case() {
        let json = r#"{"hook_event_name": "Stop"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "Stop");
    }

    #[test]
    fn test_should_process_codex_event() {
        let input = CodexNotifyInput {
            event_type: "agent-turn-complete".to_string(),
            thread_id: Some("thread-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: Some("D:/workspace".to_string()),
            input_messages: vec!["请修复测试".to_string()],
            last_assistant_message: Some("已修复并完成验证。".to_string()),
        };

        assert!(should_process_codex(&input));
    }

    #[test]
    fn test_should_skip_non_completion_codex_event() {
        let input = CodexNotifyInput {
            event_type: "approval-requested".to_string(),
            thread_id: Some("thread-123".to_string()),
            turn_id: None,
            cwd: None,
            input_messages: Vec::new(),
            last_assistant_message: None,
        };

        assert!(!should_process_codex(&input));
    }

    #[test]
    fn test_build_codex_turns() {
        let input = CodexNotifyInput {
            event_type: "agent-turn-complete".to_string(),
            thread_id: Some("thread-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: None,
            input_messages: vec!["请分析失败原因".to_string(), "再补一个最小测试".to_string()],
            last_assistant_message: Some("已经修复完成。".to_string()),
        };

        let turns = build_codex_turns(&input);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[1].content, "再补一个最小测试");
        assert_eq!(turns[2].role, "assistant");
        assert_eq!(turns[2].content, "已经修复完成。");
    }

    #[test]
    fn test_build_codex_turns_without_last_assistant_message() {
        let input = CodexNotifyInput {
            event_type: "agent-turn-complete".to_string(),
            thread_id: None,
            turn_id: None,
            cwd: None,
            input_messages: Vec::new(),
            last_assistant_message: None,
        };

        let turns = build_codex_turns(&input);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, "assistant");
        assert!(turns[0]
            .content
            .contains("Codex 未附带 last-assistant-message"));
    }

    #[test]
    fn test_build_codex_dedup_key() {
        let input = CodexNotifyInput {
            event_type: "agent-turn-complete".to_string(),
            thread_id: Some("thread-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: None,
            input_messages: Vec::new(),
            last_assistant_message: None,
        };

        assert_eq!(
            build_codex_dedup_key(&input).as_deref(),
            Some("codex:thread-123:turn-456")
        );
    }

    #[test]
    fn test_deserialize_codex_notify_input() {
        let json = r#"{
            "type": "agent-turn-complete",
            "thread-id": "thread-123",
            "turnId": "turn-456",
            "cwd": "D:/workspace",
            "input-messages": ["请分析这个项目", "然后修改代码"],
            "last-assistant-message": "已经完成修改。"
        }"#;

        let input = parse_codex_notify_input(json).unwrap();
        assert_eq!(input.event_type, "agent-turn-complete");
        assert_eq!(input.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(input.turn_id.as_deref(), Some("turn-456"));
        assert_eq!(input.input_messages.len(), 2);
        assert_eq!(
            input.last_assistant_message.as_deref(),
            Some("已经完成修改。")
        );
    }
}
