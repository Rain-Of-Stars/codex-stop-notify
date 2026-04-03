# codex-stop-notify

> Codex 任务结束自动邮件通知工具

当 Codex 通过 notify = [] 发出 agent-turn-complete 事件时，本工具会把本轮用户输入和最后一条助手回复整理成 HTML 邮件并发到你的邮箱。项目同时保留了旧 Hook 兼容模式，方便平滑迁移。

---

## 功能特性

| 特性 | 说明 |
|------|------|
| 自动触发 | 直接接入 Codex 的 notify = [] 外部程序机制 |
| 双模式兼容 | 优先支持 Codex notify，同时兼容旧 Hook |
| HTML 邮件 | 以对话卡片形式展示用户消息和助手回复 |
| 自动脱敏 | 自动隐藏邮箱、本机用户名和绝对路径中的敏感片段 |
| 幂等去重 | 基于线程/轮次标识和内容指纹防止重复发信 |
| 零依赖部署 | 单一可执行文件，无需额外运行时 |
| 跨平台 | 支持 Windows、Linux 和 macOS（x86_64 / Apple Silicon） |

---

## 工作原理

```text
Codex 完成一轮任务
        │
        ▼ notify = []
 codex-stop-notify (本工具)
        │
        ├─ 读取 Codex 追加的 JSON 参数
        ├─ 过滤：仅处理 agent-turn-complete
        ├─ 提取 input-messages / last-assistant-message
        ├─ 敏感信息脱敏
        ├─ 渲染 HTML 邮件
        ├─ 幂等去重检查
        └─ SMTP 发送邮件
```

兼容模式下，本工具仍可读取旧 Hook 的 stdin 与 transcript，并沿用原来的稳定性检查和 transcript 白名单逻辑。

---

## 快速开始

### 第一步：下载并解压 ZIP 安装包

从 [Releases 页面](../../releases) 下载对应平台的 ZIP 安装包：

| 平台 | 文件名 |
|------|--------|
| Windows (x86_64) | codex-stop-notify-windows-x86_64.zip |
| Linux (x86_64) | codex-stop-notify-linux-x86_64.zip |
| macOS (Intel) | codex-stop-notify-macos-x86_64.zip |
| macOS (Apple Silicon) | codex-stop-notify-macos-arm64.zip |

把 ZIP 直接解压到 `.codex` 目录。压缩包根目录已经是 `.codex` 内部结构，解压后就能直接得到 `notify/` 和 `config.toml.example`。

Windows（PowerShell）：

```powershell
Expand-Archive codex-stop-notify-windows-x86_64.zip -DestinationPath "$env:USERPROFILE\.codex" -Force
```

Linux / macOS：

```bash
mkdir -p ~/.codex
unzip -o codex-stop-notify-linux-x86_64.zip -d ~/.codex
chmod +x ~/.codex/notify/codex-stop-notify
```

解压后目录结构如下：

```text
config.toml.example
notify/
├── codex-stop-notify[.exe]
└── codex-stop-notif.env.example
```

### 第二步：填写 SMTP 配置

将 env 示例文件复制为 codex-stop-notif.env，然后填入真实 SMTP 参数。

Windows（PowerShell）：

```powershell
Copy-Item "$env:USERPROFILE\.codex\notify\codex-stop-notif.env.example" `
          "$env:USERPROFILE\.codex\notify\codex-stop-notif.env"
```

Linux / macOS：

```bash
cp ~/.codex/notify/codex-stop-notif.env.example \
   ~/.codex/notify/codex-stop-notif.env
```

最小配置示例：

```dotenv
SMTP_HOST=smtp.qq.com
SMTP_PORT=465
SMTP_USER=your_qq@qq.com
SMTP_PASSWORD=your_smtp_authorization_code
SMTP_USE_SSL=true
EMAIL_TO=your_qq@qq.com
```

提示：这里的密码通常是 SMTP 授权码，不是邮箱网页登录密码。

### 第三步：配置 Codex notify

打开 ~/.codex/config.toml（不存在就新建），加入 notify 配置。Codex 会在命令行参数末尾自动追加一个 JSON 字符串，本工具会自行解析它。下面示例统一使用环境变量路径，避免把用户名写死在配置里。

Windows：

```toml
notify = [
  "pwsh",
  "-NoProfile",
  "-CommandWithArgs",
  '& "$env:USERPROFILE\.codex\notify\codex-stop-notify.exe" --env-file "$env:USERPROFILE\.codex\notify\codex-stop-notif.env" $args[0]'
]
```

Linux / macOS：

```toml
notify = [
  "sh",
  "-lc",
  '"$HOME/.codex/notify/codex-stop-notify" --env-file "$HOME/.codex/notify/codex-stop-notif.env" "$1"',
  "codex-notify-shell"
]
```

说明：Windows 示例依赖 `pwsh` 的 `-CommandWithArgs`；Linux / macOS 示例里的 `"codex-notify-shell"` 是给 `sh -lc` 占位用的 `$0`，不要删掉；Codex 追加的 JSON 会落到 `$1`，脚本再把它转发给本工具。

仓库里也提供了示例文件 hooks/codex-config.toml.example，可直接参考。

### 第四步：验证触发

完成配置后，运行一轮 Codex 任务。任务结束并产生 agent-turn-complete 事件时，就会自动发信。

如果你想手动验证外部程序链路，可以直接传入一个模拟 JSON：

```powershell
& "$env:USERPROFILE\.codex\notify\codex-stop-notify.exe" `
  --env-file "$env:USERPROFILE\.codex\notify\codex-stop-notif.env" `
  '{"type":"agent-turn-complete","thread-id":"demo-thread","turn-id":"demo-turn","cwd":"D:/demo","input-messages":["请总结这次修改"],"last-assistant-message":"已完成修改并通过测试。"}'
```

---

## 配置参考

所有配置项都写在 env 文件中。

### SMTP 配置

| 配置项 | 必填 | 默认值 | 说明 |
|--------|:----:|--------|------|
| SMTP_HOST | 是 | - | SMTP 服务器地址，例如 smtp.qq.com |
| SMTP_PORT | 是 | - | SMTP 端口，SSL 常用 465，STARTTLS 常用 587 |
| SMTP_USER | 是 | - | SMTP 用户名，通常是完整邮箱地址 |
| SMTP_PASSWORD | 是 | - | SMTP 授权码或密码 |
| SMTP_USE_SSL | 否 | true | 是否使用 SSL/TLS 连接 |
| SMTP_ALLOW_INSECURE_PLAIN | 否 | false | 是否允许明文认证，默认不推荐 |

### 邮件配置

| 配置项 | 必填 | 默认值 | 说明 |
|--------|:----:|--------|------|
| EMAIL_FROM | 否 | SMTP_USER | 发件人地址 |
| EMAIL_TO | 是 | - | 收件人，多个地址用英文逗号分隔 |
| EMAIL_INCLUDE_CONTEXT | 否 | false | 是否附带工作目录、线程 ID 等上下文 |

### 兼容模式高级配置

| 配置项 | 必填 | 默认值 | 说明 |
|--------|:----:|--------|------|
| TRANSCRIPT_ALLOWED_ROOTS | 否 | 系统默认 | 仅在兼容模式下使用，用于扩展 transcript 读取白名单 |

完整示例：

```dotenv
SMTP_HOST=smtp.example.com
SMTP_PORT=465
SMTP_USER=your_email@example.com
SMTP_PASSWORD=your_smtp_password
SMTP_USE_SSL=true

# EMAIL_FROM=notify@example.com
EMAIL_TO=you@example.com,team@example.com
EMAIL_INCLUDE_CONTEXT=false

# 仅在兼容模式下需要
# TRANSCRIPT_ALLOWED_ROOTS=%LOCALAPPDATA%\legacy-hook-logs;D:\custom-path
```

---

## 常用邮箱 SMTP 配置速查

QQ 邮箱：

```dotenv
SMTP_HOST=smtp.qq.com
SMTP_PORT=465
SMTP_USER=your_qq@qq.com
SMTP_PASSWORD=<授权码>
SMTP_USE_SSL=true
```

163 / 126 邮箱：

```dotenv
SMTP_HOST=smtp.163.com
SMTP_PORT=465
SMTP_USER=your_email@163.com
SMTP_PASSWORD=<授权码>
SMTP_USE_SSL=true
```

Gmail：

```dotenv
SMTP_HOST=smtp.gmail.com
SMTP_PORT=465
SMTP_USER=your_email@gmail.com
SMTP_PASSWORD=<应用专用密码>
SMTP_USE_SSL=true
```

Outlook / Hotmail：

```dotenv
SMTP_HOST=smtp.office365.com
SMTP_PORT=587
SMTP_USER=your_email@outlook.com
SMTP_PASSWORD=<邮箱密码>
SMTP_USE_SSL=false
```

企业邮箱（腾讯企业邮箱）：

```dotenv
SMTP_HOST=smtp.exmail.qq.com
SMTP_PORT=465
SMTP_USER=your_email@your_company.com
SMTP_PASSWORD=<邮箱密码>
SMTP_USE_SSL=true
```

---

## 从源码构建

需要 Rust stable 工具链（建议 1.75+）。

```bash
git clone https://github.com/your-username/codex-stop-notify.git
cd codex-stop-notify
cargo build --release
```

构建产物位置：

```text
Windows: target\release\codex-stop-notify.exe
Linux:   target/release/codex-stop-notify
macOS:   target/release/codex-stop-notify
```

运行测试：

```bash
cargo test
```

---

## 安全说明

- 配置文件 codex-stop-notif.env 包含 SMTP 密码，请限制为当前用户可读。
- 即使 EMAIL_INCLUDE_CONTEXT=true，邮件中也会自动脱敏邮箱、本机用户名和绝对路径中的敏感片段。
- Transcript 白名单只在 兼容模式下生效，用于拒绝任意路径读取。
- 本地自行构建的二进制可能包含调试路径，分发时建议优先使用 Release 页中的 CI 构建产物。

---

## 故障排查

问题：Codex 任务结束后没有收到邮件

1. 确认 ~/.codex/config.toml 中的 notify 配置能够在当前 shell 中正确展开到可执行文件和 env 文件。
2. 确认 codex-stop-notif.env 存在且 SMTP 参数填写正确。
3. 直接在终端手动执行一次 --notify-payload 示例，确认外部程序本身能发信。
4. 如果你使用的是旧 Hook，请确认兼容模式路径和 transcript 白名单仍然可用。

问题：提示 SMTP 认证失败

1. 确认使用的是 SMTP 授权码，而不是邮箱网页登录密码。
2. 确认邮箱后台已开启 SMTP/IMAP 服务。

问题：同一轮任务重复发信

1. 先检查系统临时目录中的去重目录是否可写：%TEMP%\codex-stop-notify-dedup。
2. 如果你手动重复调用同一份 JSON 负载，命中去重是预期行为。

问题：邮件内容被截断

1. 单轮消息超过 20,000 字符时会截断。
2. 总邮件超过 220,000 字符时会停止继续渲染后续内容，这是为了兼容常见邮件客户端。

---

## 兼容说明

本项目现在以 Codex notify 为主入口，但没有移除旧 Hook 处理链路。如果你还在迁移过程中，原来的 stdin + transcript 模式仍可继续使用。

---

## License

MIT
