# SecureChat — Tauri 端到端加密聊天桌面应用设计

## 概述

SecureChat 用于两台机器之间的秘密通话。Tauri 版采用 **React + TypeScript** 界面、**Tauri 2** 桌面壳和独立 **Rust Core**，保持既有 v1 线路协议不变，可与旧 WinUI/.NET 客户端互通。

- **传输**：TCP 直连全双工；创建方监听，加入方通过房间码连接。
- **加密**：P-256 ECDH 临时密钥 + PSK 认证 + HKDF-SHA256 + AES-256-GCM，并定期轮换密钥。
- **界面**：聊天窗口只显示解密后的明文；消息、口令和密钥均不持久化。
- **桌面集成**：关窗隐藏到托盘，后台消息通过系统通知、任务栏提醒、标题未读计数和提示音告知用户。
- **进程边界**：`crates/securechat-core` 不依赖 Tauri；`src-tauri` 通过 commands/events 桥接 React；Rust 与 React 均为单应用内组件，无 .NET Sidecar。

## 协议与安全设计

### 帧格式

线路帧采用大端序：

```text
[4B total_len][1B type][4B epoch][payload]
```

- 头部 9 字节，`total_len` 包含头部；载荷上限 1 MiB。
- reader 必须完整读取头部和载荷，正确处理 TCP 半包与粘包。
- 帧类型：`1=Hello`、`2=Confirm`、`3=Data`、`4=Profile`、`5=Rekey`、`6=Ping`、`7=Bye`、`8=RekeyRequest`、`9=Over`。
- Data、Profile、Ping、Rekey、RekeyRequest 和 Over 在建立会话后使用对应纪元密钥保护；Hello/Confirm 用于握手，Bye 用于尽力通知断开。

### 房间码

```text
"SC1-" + Base64Url(
  ver(1B)=1 | ipLen(1B) | ip(4/16B) |
  port(2B BE) | rotation(2B BE) | psk(32B)
)
```

- PSK 由 CSPRNG 生成 32 字节，杜绝人工弱口令。
- 解析严格校验版本、精确长度、IPv4/IPv6 地址、端口 1–65535 和周期 1–720 分钟；容忍前后空白与大小写前缀。
- Core 握手接收的 PSK 是 **43 字符 Base64Url 文本**。HKDF 使用该文本的 UTF-8 字节，不是解码后的 32 字节。
- 房间码包含全部连接秘密，必须经可信渠道传递。
- 本机候选地址排除回环和链路本地 IPv4，私网优先；没有可用地址时回退到 `127.0.0.1`。

### 握手

```text
A（连接方）                                      B（监听方）
  │ Hello: ver(1B) ‖ pubLen(2B) ‖ pubA(SPKI) ‖ nonceA(16B) │
  │──────────────────────────────────────────────────────────▶│
  │◀──────────────────────── Hello(pubB, nonceB)──────────────│
  Z = ECDH(ephA, ephB)
  transcript = SHA256(pubA ‖ nonceA ‖ pubB ‖ nonceB)
  HKDF-SHA256(
    ikm = Z ‖ UTF8(pskText),
    salt = transcript,
    info = "SecureChat/v1/epoch0",
    length = 96
  )
  │ Confirm: mac(32B) ‖ tiebreak(8B)                         │
  │──────────────────────────────────────────────────────────▶│
  │◀──────────────────── Confirm（对向同理）─────────────────│
  ▼ 进入加密会话，发送加密 Profile
```

- 临时公钥使用 P-256 SPKI DER；ECDH 使用原始共享秘密。
- 96 字节 OKM 依次划分为 `keyA2B`、`keyB2A`、`confirmKey`，各 32 字节。
- 每方向 nonce 基值分别为 `HMAC(confirmKey, "nonce-base-a"/"nonce-base-b")` 的前 8 字节。
- Confirm MAC = `HMAC(confirmKey, transcript ‖ role)`，role 为 1（A）或 2（B），并恒时校验。
- PSK 错误或中间人无法生成有效 Confirm；握手整体 10 秒超时。
- 双方各生成 8 字节 tiebreak，用于 Rekey 碰撞仲裁。
- 显示名只在握手完成后通过加密 Profile JSON `{name: "..."}` 传输。

### 消息加密

- A 为连接方，B 为监听方；两个方向使用独立密钥与 nonce 基值。
- 密文载荷：`[4B counter][ciphertext][16B GCM tag]`。
- nonce：`8B nonceBase ‖ 4B counter(BE)`；计数器每个纪元从 1 开始。
- AAD：`type(1B) ‖ epoch(4B) ‖ counter(4B) ‖ plainLen(4B)`，共 13 字节。
- 接收端按 epoch 保存最后接收计数器，要求严格递增；重复、乱序或篡改都会关闭连接。
- 密钥指纹为 `SHA256(keyA2B ‖ keyB2A)` 前 8 字节的小写十六进制，供双方人工比对。
- 加密 JSON 字段保持：聊天 `{content: "..."}`、显示名 `{name: "..."}`、OVER `{kind: "..."}`。

### 密钥轮换（Rekey）

- 默认 10 分钟，允许 1–720 分钟；支持手动和自动轮换，单次轮换 10 秒超时。
- tiebreak 字节序较低方为 driver：只有 driver 能主动发送 RekeyHello，高方发送空载荷 RekeyRequest 请求 driver 发起。
- 高方收到 driver Hello 后，用旧纪元密钥回发自己的 Hello，再切换到新纪元；driver 收到响应后切换并发送新纪元 Ping 回执。
- 双方同时操作时按同一角色规则收敛，迟到旧纪元的重复 Rekey 被忽略。
- 新 transcript：

```text
SHA256(oldTranscript ‖ pubLow ‖ nonceLow ‖ pubHigh ‖ nonceHigh)
```

- 新 HKDF info 为 `SecureChat/v1/epoch{n}`；双向密钥、nonce 基值和计数器随纪元更新。
- 切换时保留上一接收纪元一轮，允许轮换边界处的在途旧帧解密。
- 发送顺序、密钥切换和计数器由同一 Actor 串行化；不得持锁跨网络 `await`。
- Ping 每 25 秒按需发送，90 秒没有任何收包判定连接死亡；断开时尽力发送 Bye。

### OVER 协商

OVER 使用加密帧类型 9，载荷 `{kind}` 支持：

- `request`：请求结束；本方进入等待，对端显示确认卡片。
- `accept`：接受结束；双方展示短暂告别后清空消息并断开。
- `reject`：拒绝结束，双方继续聊天。
- `cancel`：撤回请求，对端移除确认卡片。

双方同时发出 request 时视为共同接受，各自回应 accept 并结束。重复结束流程必须幂等，断开后旧会话事件不得影响新会话。

## Tauri 桥接与桌面生命周期

- Commands：枚举 IP、创建/加入房间、断开、发送消息、立即换钥、OVER 操作和获取快照。
- 创建房间在监听绑定成功后立即返回 `{sessionId, roomCode}`，accept/握手在后台继续，保证等待期间能复制房间码和取消。
- 统一 `session-event` 携带 `sessionId`、单调 `seq`、`kind` 和 `payload`；React reducer 丢弃旧会话与重复/乱序事件，快照用于补齐订阅前状态。
- 事件覆盖状态、消息、对端昵称、密钥轮换、OVER 和断开；断开只发送一次并作为终止事件。
- 关闭窗口只隐藏，Tokio 连接和计时器继续运行；托盘双击或菜单恢复窗口，只有托盘“退出”才真正结束。
- 后台收到消息时，系统通知只显示发送者，不包含消息正文；同时更新未读标题、请求任务栏 attention 并播放提示音，回前台清零。
- 真正退出时取消连接与后台任务并清理密钥。日志不得记录房间码、PSK、消息明文或密钥。

> 界面结构、交互约定与动效设计语言见 [ux.md](./ux.md)。

## 项目结构

```text
SecureChat/
├─ Cargo.toml                         # Rust workspace
├─ crates/securechat-core/
│  ├─ src/protocol/                   # frame、room_code、payload
│  ├─ src/crypto/                     # handshake、session
│  └─ src/connection/                 # actor、rekey、事件与配置
├─ src-tauri/
│  ├─ src/                            # commands、事件桥、桌面集成
│  ├─ capabilities/                   # 最小权限声明
│  └─ icons/                          # 复用应用图标
├─ src/                               # React + TypeScript
│  ├─ components/                     # 连接区、聊天列表与 OVER 卡片
│  ├─ hooks/                          # 会话订阅与操作
│  ├─ store.ts                        # sessionId/seq 事件 reducer
│  └─ api.ts、types.ts、App.tsx
└─ .github/workflows/release.yml      # Windows 测试、构建与发布
```

## 验证要求

1. `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。
2. `pnpm test` 与 `pnpm build`。
3. Rust 与旧 .NET 客户端双角色互操作：双方分别创建/加入，验证双向消息和双方发起 Rekey。
4. 两个 Rust 实例验证聊天、自动/手动/同时 Rekey、OVER、异常断开和防重放。
5. `pnpm tauri build --no-bundle` 构建 Windows 可执行文件，并验证托盘、后台通知、取消监听和窗口最小尺寸。

## 部署与边界

- Windows 发布产物为 `target/release/securechat.exe`；不需要 .NET 或 Rust 运行时，但依赖系统 WebView2 Runtime。
- 不保存历史消息，不做定长填充或流量分析防护，不提供账号、中继、多会话和 NAT 穿透。
- 点对点直连跨公网使用时，端口映射、防火墙和对方可达地址由用户负责。
