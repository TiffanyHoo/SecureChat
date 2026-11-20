# SecureChat

基于 **Tauri 2 + React + TypeScript + Rust** 的端到端加密聊天桌面应用。两台机器通过 TCP 直连秘密通话，线路全程密文（含显示名），界面只显示解密后的明文。

## 特性

- **端到端加密**：ECDH P-256 临时密钥协商 + 256 位 PSK 认证 + HKDF-SHA256 + AES-256-GCM；姓名与消息不会以明文出现在网络中。
- **房间码一键入会**：创建房间时把 IP、端口、换钥周期和随机口令打包为 `SC1-` 房间码，对方粘贴即可加入。
- **会话密钥自动轮换**：默认每 10 分钟（可设 1–720 分钟）执行新一轮 ECDH；界面显示纪元、密钥指纹和倒计时，也可立即换钥。
- **防篡改防重放**：帧头通过 AAD 绑定到 GCM 认证，接收计数器严格递增，篡改、重复或乱序帧会中止连接。
- **OVER 双向协商**：双方都同意才结束通话，支持同时请求、拒绝和撤回。
- **后台驻留提醒**：关闭窗口会隐藏到系统托盘；后台收到消息时显示不含正文的系统通知、未读计数、任务栏提醒和提示音。
- **零落盘**：不保存聊天记录和会话密钥，真正退出后全部清除。

## 构建与运行

要求：

- Windows 10/11，系统具备 WebView2 Runtime
- Rust stable（含 `cargo`、`rustfmt`、`clippy`）
- Node.js 20+ 与 pnpm 11

```powershell
pnpm install
pnpm tauri dev
```

构建 Windows 便携可执行文件：

```powershell
pnpm tauri build --no-bundle
# 产物：target/release/securechat.exe
```

目标机器不需要安装 Rust 或 .NET，但需要 WebView2 Runtime。若需 MSI/NSIS 安装包，可执行 `pnpm tauri build`。

## 使用方法

1. **创建房间**（一方）：填显示名，选择对方可达的本机 IP，按需调整端口和换钥周期，点击「创建房间」。
2. 房间监听成功后立即生成房间码；点击「复制房间码」，通过可信渠道发给对方。等待期间可以取消监听。
3. **加入房间**（另一方）：填显示名，粘贴完整房间码，点击「加入房间」。
4. 握手完成后即可双向聊天。底部状态条显示密钥纪元、指纹和换钥倒计时。
5. 关闭窗口只会隐藏到托盘；双击托盘图标恢复，托盘右键菜单选择「退出」才会真正结束程序。

> 房间码内含会话口令，码即口令，务必通过可信渠道传递。点对点直连跨公网使用时，需要自行处理端口映射和防火墙放行。

## 测试与检查

```powershell
pnpm test                 # React reducer 与交互测试
pnpm build                # TypeScript 检查与前端生产构建
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace    # 协议、加密与 TCP 回环测试
```

## 项目结构

| 路径 | 说明 |
|---|---|
| `crates/securechat-core` | 独立 Rust Core：帧、房间码、ECDH/HKDF、AES-GCM、连接 Actor、Rekey 与保活 |
| `src-tauri` | Tauri 壳、commands/events、托盘、窗口与系统通知 |
| `src` | React + TypeScript 单页界面、会话 reducer 与组件测试 |
| `docs/design.md` | v1 线路协议和安全设计 |

Tauri 版保持 v1 线路协议不变，可与旧 WinUI/.NET 版本互通。完整协议设计见 [docs/design.md](docs/design.md)。
