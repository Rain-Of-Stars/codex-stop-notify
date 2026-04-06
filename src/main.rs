// codex-stop-notify: Codex 任务结束后自动发送邮件通知
// 优先处理 Codex notify = [] 传入的 JSON 参数，并兼容旧 Hook 的 stdin 输入

mod config;
mod dedup;
mod email;
mod event;
mod html;
mod redact;
mod transcript;

use chrono::Local;
use redact::{redact_sensitive_text, safe_prefix, summarize_path_for_display};
use std::path::PathBuf;
use std::process;
use transcript::Turn;

/// 命令行参数
struct CliArgs {
    env_file: Option<PathBuf>,
    notify_payload: Option<String>,
}

/// 解析命令行参数
fn parse_cli_args() -> Result<CliArgs, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut env_file = None;
    let mut notify_payload = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--env-file" => {
                let path = args
                    .get(i + 1)
                    .ok_or_else(|| "--env-file 缺少路径参数".to_string())?;
                env_file = Some(PathBuf::from(path));
                i += 2;
            }
            "--notify-payload" => {
                let payload = args
                    .get(i + 1)
                    .ok_or_else(|| "--notify-payload 缺少 JSON 参数".to_string())?;
                notify_payload = Some(payload.clone());
                i += 2;
            }
            value if notify_payload.is_none() && value.trim_start().starts_with('{') => {
                notify_payload = Some(value.to_string());
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    Ok(CliArgs {
        env_file,
        notify_payload,
    })
}

/// 解析配置文件路径
fn resolve_env_path(explicit_env_file: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit_env_file {
        if path.is_file() {
            return Ok(path);
        }

        return Err(format!(
            "指定的 env 文件不存在: {}",
            summarize_path_for_display(&path)
        ));
    }

    config::find_env_file()
}

fn main() {
    // 外部通知程序应尽量以 0 退出，避免阻塞 Codex 或兼容 Hook 的正常流程
    match run() {
        Ok(()) => {
            println!("{{}}");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("[codex-stop-notify 错误] {}", redact_sensitive_text(&e));
            println!("{{}}");
            process::exit(0);
        }
    }
}

fn run() -> Result<(), String> {
    let cli = parse_cli_args()?;
    let env_path = resolve_env_path(cli.env_file)?;
    let cfg = config::Config::load(&env_path)?;

    if let Some(payload) = cli.notify_payload.as_deref() {
        let input = event::parse_codex_notify_input(payload)?;
        return process_codex_notification(&cfg, &input);
    }

    let input = event::read_hook_input()?;
    process_legacy_hook(&cfg, &input)
}

fn process_codex_notification(
    cfg: &config::Config,
    input: &event::CodexNotifyInput,
) -> Result<(), String> {
    if !event::should_process_codex(input) {
        let reason = event::codex_skip_reason(input).unwrap_or("未命中发送条件");
        eprintln!(
            "[codex-stop-notify] 跳过 Codex 事件: {} ({})",
            input.event_type, reason
        );
        return Ok(());
    }

    let id_for_log = input
        .thread_id
        .as_deref()
        .or(input.turn_id.as_deref())
        .map(|value| safe_prefix(value, 12))
        .unwrap_or_else(|| "未知".to_string());
    eprintln!(
        "[codex-stop-notify] 处理 Codex notify 事件，标识: {}",
        id_for_log
    );

    let turns = event::build_codex_turns(input);
    if turns.is_empty() {
        eprintln!("[codex-stop-notify] Codex notify 无有效内容，跳过发送");
        return Ok(());
    }

    let fingerprint = transcript::turns_fingerprint(&turns);
    let dedup_key = event::build_codex_dedup_key(input);
    let turn_count = turns.len();
    if let Some(key) = dedup_key.as_deref() {
        if dedup::is_duplicate(key, turn_count, &fingerprint) {
            eprintln!(
                "[codex-stop-notify] Codex 事件 {} 已发送过通知，跳过",
                safe_prefix(key, 16)
            );
            return Ok(());
        }
    }

    let timestamp = Local::now().to_rfc3339();
    let subject = build_codex_subject(input, &timestamp);
    let identifier = input.thread_id.as_deref().or(input.turn_id.as_deref());
    send_notification_email(
        cfg,
        event::NotificationKind::Codex,
        &turns,
        identifier,
        &timestamp,
        input.cwd.as_deref(),
        &subject,
    )?;

    if let Some(key) = dedup_key.as_deref() {
        dedup::mark_sent(key, turn_count, &fingerprint)?;
    }

    Ok(())
}

fn process_legacy_hook(cfg: &config::Config, input: &event::HookInput) -> Result<(), String> {
    if !event::should_process(input)? {
        eprintln!(
            "[codex-stop-notify] 跳过兼容 Hook 事件: {} (stop_hook_active={:?})",
            input.hook_event_name, input.stop_hook_active
        );
        return Ok(());
    }

    let session_for_log = input
        .session_id
        .as_deref()
        .map(|sid| safe_prefix(sid, 12))
        .unwrap_or_else(|| "未知".to_string());
    eprintln!(
        "[codex-stop-notify] 兼容处理旧 Hook Stop 事件，会话: {}",
        session_for_log
    );

    let transcript_str = input.transcript_path.as_deref().unwrap();
    let transcript_path = transcript::validate_path(transcript_str, &cfg.transcript.allowed_roots)?;
    let snapshot = transcript::wait_for_complete_transcript(&transcript_path)?;

    let turns = snapshot.turns;
    if turns.is_empty() {
        eprintln!("[codex-stop-notify] transcript 无有效内容，跳过发送");
        return Ok(());
    }

    eprintln!("[codex-stop-notify] 解析到 {} 轮对话", turns.len());

    if transcript::is_subagent_iteration(&turns) {
        eprintln!(
            "[codex-stop-notify] 检测到子智能体 </final_answer> 收尾，跳过本次发送并保留后续主会话通知机会"
        );
        return Ok(());
    }

    let turn_count = turns.len();
    if let Some(ref sid) = input.session_id {
        if dedup::is_duplicate(sid, turn_count, &snapshot.fingerprint) {
            eprintln!(
                "[codex-stop-notify] 会话 {} 已发送过通知（轮次数 {} 与内容指纹均未变化），跳过",
                safe_prefix(sid, 12),
                turn_count
            );
            return Ok(());
        }
    }

    let subject = build_legacy_subject(input);
    send_notification_email(
        cfg,
        event::NotificationKind::LegacyHook,
        &turns,
        input.session_id.as_deref(),
        input.timestamp.as_deref().unwrap_or("未知时间"),
        input.cwd.as_deref(),
        &subject,
    )?;

    if let Some(ref sid) = input.session_id {
        dedup::mark_sent(sid, turn_count, &snapshot.fingerprint)?;
    }

    Ok(())
}

fn send_notification_email(
    cfg: &config::Config,
    kind: event::NotificationKind,
    turns: &[Turn],
    identifier: Option<&str>,
    timestamp: &str,
    cwd: Option<&str>,
    subject: &str,
) -> Result<(), String> {
    let email_html = html::render_email_html_with_kind(
        kind,
        turns,
        identifier,
        Some(timestamp),
        cwd,
        cfg.email.include_context,
    );

    email::send_email(
        &cfg.smtp,
        &cfg.email.from,
        &cfg.email.to,
        subject,
        &email_html,
    )?;

    let recipients: Vec<String> = cfg
        .email
        .to
        .iter()
        .map(|addr| redact_sensitive_text(addr))
        .collect();
    let source_name = match kind {
        event::NotificationKind::Codex => "Codex",
        event::NotificationKind::LegacyHook => "兼容 Hook 模式",
    };
    eprintln!(
        "[codex-stop-notify] {} 邮件发送成功，收件人: {:?}",
        source_name, recipients
    );

    Ok(())
}

/// 构建兼容 Hook 模式邮件主题
fn build_legacy_subject(input: &event::HookInput) -> String {
    let time_part = input
        .timestamp
        .as_deref()
        .and_then(|value| value.split('T').next())
        .unwrap_or("未知时间");

    let session_part = input
        .session_id
        .as_deref()
        .map(|value| safe_prefix(value, 8))
        .unwrap_or_else(|| "未知".to_string());

    format!("[兼容会话回顾] {} ({})", time_part, session_part)
}

/// 构建 Codex 邮件主题
fn build_codex_subject(input: &event::CodexNotifyInput, timestamp: &str) -> String {
    let time_part = timestamp.split('T').next().unwrap_or("未知时间");
    let thread_part = input
        .thread_id
        .as_deref()
        .map(|thread| safe_prefix(thread, 8))
        .unwrap_or_else(|| "未知线程".to_string());
    let turn_part = input
        .turn_id
        .as_deref()
        .map(|turn| format!("/{}", safe_prefix(turn, 8)))
        .unwrap_or_default();

    format!(
        "[Codex 任务通知] {} ({}{})",
        time_part, thread_part, turn_part
    )
}
