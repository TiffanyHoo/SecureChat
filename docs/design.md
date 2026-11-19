# SecureChat — WinUI 3 端到端加密聊天桌面应用设计文档

> **状态：已实现并验证**。全解决方案构建 0 警告 0 错误；测试套件 19/19 全绿。
> 本文档为最终设计，与代码一致；标注 [实现新增] 的条目为实施过程中确定的设计。

## 概述

为"两台机器之间秘密通话"场景设计的点对点加密聊天应用：

- **前端**：WinUI 3（C# / .NET 8），聊天界面始终显示解密后的明文。
- **传输**：TCP 直连全双工（一端创建房间监听，另一端粘贴房间码连入，连接建立后双向互发互收）。
- **加密**：ECDH P-256 临时密钥协商 + PSK 口令认证 + AES-256-GCM 消息加密 + 定期自动轮换会话密钥。链路上（含握手）不出现姓名与消息明文，抓包者无法还原任何内容。
- **通知** [实现新增]：窗口后台时收到消息弹系统 Toast（仅提示来源，不含消息内容）+ 任务栏闪烁 + 未读计数 + 提示音。

技术栈：`net8.0-windows10.0.19041.0` + WindowsAppSDK 2.6.4 非打包部署（unpackaged、self-contained、x64，目标机器免装 .NET），Core + App + Tests 三工程结构。

## 协议与安全设计

### 握手（防中间人 + 防明文）

```
A(发起方/连接方)                          B(监听方)
  │ Hello: ver(1B) ‖ ephPubA(SPKI) ‖ nonceA(16B)     │
  │──────────────────────────────────────────────────▶│
  │        Hello: ver ‖ ephPubB ‖ nonceB              │
  │◀─────────────────────────────────────────────────│
  Z = ECDH(ephA, ephB)  [DeriveRawSecretAgreement]
  transcript = SHA256(ephPubA ‖ nonceA ‖ ephPubB ‖ nonceB)   // 发起方视角排序
  HKDF-SHA256(ikm = Z ‖ PSK, salt = transcript, info = "SecureChat/v1/epoch0")
        → 96B OKM: keyA2B(32B) / keyB2A(32B) / confirmKey(32B)
        → nonce 基值 = HMAC(confirmKey, ...) 取前 8B（每方向）
  │ Confirm: mac(32B) ‖ tiebreak(8B 随机)   [实现新增] │
  │───────────────────────────────────────────────────▶│
  │◀──────────────── Confirm(对向同理) ────────────────│
  ▼ 握手完成，进入加密通道；双方互发 Profile(加密) 携带显示名
```

- PSK 错误 → Confirm MAC 校验失败（`FixedTimeEquals` 恒时比较）→ 断开并提示"口令不匹配"。PSK 同时作为 HKDF 输入，使会话密钥绑定双方共知口令，杜绝中间人。
- **姓名只在握手完成后的加密 `Profile` 帧中传输**（JSON `{n: 显示名}`），抓包全程不可见。
- 握手报文只有临时公钥与随机数，无任何可识别身份信息。握手整体 10s 超时。
- **tiebreak（8B 随机数）** [实现新增]：双方各在 Confirm 中携带；用于密钥轮换时确定性分配角色（见下），比较按字节字典序。

### 消息加密

- 每方向独立密钥（keyA2B / keyB2A）；nonce 96 位 = **8B 随机基值（confirmKey 派生）+ 4B 单调递增计数器**。
- **防重放**：接收端按 epoch 维护计数器表，校验计数器严格递增，乱序/重复即拒绝。
- 密文格式：`[4B counter][AES-256-GCM 密文][16B tag]`；nonce 基值轮换时重新派生。
- AAD = `type(1B) ‖ epoch(4B) ‖ counter(4B) ‖ plainLen(4B)`（13B），篡改任何头部字段都会导致 GCM 认证失败。
- 帧格式（大端）：`[4B 总长][1B type][4B epoch][payload]`，头 9B，载荷上限 1MiB；`ReadExactAsync` 处理 TCP 粘包/半包。
- 帧类型：`1=Hello, 2=Confirm, 3=Data, 4=Profile, 5=Rekey, 6=Ping, 7=Bye, 8=RekeyRequest`（8 为 [实现新增]）。

### 密钥轮换（Rekey）[实现修订]

- 周期默认 **10 分钟**，UI 可设 **1–720 分钟**；提供"立即换钥"按钮。
- **确定性角色**（消除双方同时轮换的竞态）：
  - tiebreak 较低方 = **driver**，唯一有权发送 `RekeyHello`（新临时公钥 + nonce）；
  - 较高方只可发送 `RekeyRequest`（空载荷）提示 driver 发起；
  - 收到对端 RekeyHello 的高方用**旧钥**回发自己的 hello，随后双方独立切换到新纪元；
  - driver 切换后发送 `Ping` 作为新纪元回执；高方等待新纪元首帧到达即确认完成；
  - 双方同时点击"立即换钥" → 一方成为 driver、另一方退化为等待，碰撞自然收敛。
- **transcript 链式哈希**：`epochN = SHA256(旧transcript ‖ pubLow ‖ nonceLow ‖ pubHigh ‖ nonceHigh)`（按 tiebreak 排序），每轮密钥材料唯一且不可回溯。
- HKDF info = `"SecureChat/v1/epoch{n}"`，各纪元密钥完全隔离（前向保密：泄露当前密钥不影响历史）。
- **在途帧保护**：切换瞬间保留 `prevRecvKeys` 一轮，旧纪元末尾在途消息仍可解密，不丢消息。
- 保活：Ping 每 25s，90s 无收包判定掉线；断开时尽力发送 Bye。

### 房间码（连接信息打包）[实现新增]

把"创建方 IP + 监听端口 + 轮换周期 + 会话口令"编码为一段可复制的房间码，替代手动传 IP/端口/口令：

- **创建房间**：自动生成 **256 位 CSPRNG 口令**（杜绝弱口令离线穷举）→ 与选定的本机 IP、端口、轮换周期一起编码为房间码 → 一键复制发给对方 → 开始监听。
- **加入房间**：粘贴房间码 → 解析出 IP/端口/口令/周期 → 直接连入（周期跟随创建方，双方轮换节奏一致）。
- 格式：`"SC1-" + Base64Url( ver(1B)=1 | ipLen(1B) | ip(4/16B) | port(2B BE) | rotation(2B BE) | psk(32B) )`，约 60 字符。
- 解析严格校验（版本/长度/取值范围），容错前后空白与小写前缀；口令在码内以 32 字节随机数存在，对 Core 以 Base64Url 文本（43 字符）传入。
- **安全边界**：口令编码在房间码内，房间码必须经由用户信任的渠道（当面/电话等）交给对方——与"口头传口令"同一信任模型，且口令强度从"人工记忆"升级为"机器生成 256 位"。
- 多网卡（VPN/虚拟机网卡）场景：`GetCandidateHostIps()` 枚举候选 IPv4（排除回环/链路本地，私网优先排序），由创建方在界面下拉框选择。
- 实现：Core `Protocol/RoomCode.cs`（纯逻辑可测试），App 层 `CreateRoomCommand`/`JoinRoomCommand` + 复制按钮（`Clipboard`）。

### 新消息通知与后台驻留 [实现新增]

**窗口生命周期**（聊天软件惯例）：**叉掉窗口 ≠ 退出**，而是最小化到系统托盘后台驻留，继续接收加密消息并弹通知；托盘**双击恢复窗口**，**右键菜单 → "退出"** 才真正结束程序（托盘图标异常不可用时叉掉退化为正常退出，避免程序困在后台无入口）。

窗口**不在前台**时收到消息才提醒，前台聊天不打扰；回前台未读自动清零：

| 方式 | 内容 |
|---|---|
| 系统 Toast（AppNotificationBuilder） | 标题 "SecureChat"，正文"收到来自 XX 的新消息（内容已加密，请进入应用查看）"——**不含任何消息正文**；点击 Toast 将窗口带回前台（隐藏状态也可恢复）；同 Tag 通知相互替换，连收多条不刷屏 |
| 任务栏闪烁 | FlashWindowEx，标题栏 + 任务栏按钮 3 次 |
| 标题未读计数 | 窗口标题 "SecureChat 安全通话 - N 条未读" |
| 提示音 | MessageBeep |

- 首次最小化到托盘弹一次性 Toast 提示"程序在后台继续接收加密消息"。
- 非打包应用若无开始菜单快捷方式，Windows 会静默不显示 Toast，此时其余三路兜底仍生效。
- 托盘实现（WinUI 3 无内置托盘控件）：`Shell_NotifyIcon` P/Invoke + message-only 窗口接收回调；含 explorer 重启后自动重注册（TaskbarCreated 消息）。
- 相关文件：`Services/TrayIcon.cs`（托盘图标/右键菜单）、`Services/AppNotifier.cs`（Toast/点击恢复）、`MainWindow`（`AppWindow.Closing` 拦截叉掉、`RequestExit` 真退出）、`App`（ShowMainWindow/ExitApp）。

## 项目结构

```
SecureChat/
├─ SecureChat.sln                        # Core + App + Tests 三工程
├─ src/
│  ├─ SecureChat.Core/                   # net8.0 纯类库，无第三方依赖
│  │  ├─ SecureConnection.cs             # 状态机核心：握手→会话→轮换→断线（轮换/保活 Timer 内置）
│  │  │                                  #   事件: Connected/PeerNameReceived/MessageReceived/
│  │  │                                  #         KeyRotated/Disconnected；状态: Idle→Listening/Connecting
│  │  │                                  #         →Handshaking→Established→Closed
│  │  │                                  #   SecureChatOptions: RotationMinutes/KeepAliveSeconds/DeadSeconds
│  │  ├─ Crypto/
│  │  │  ├─ Handshake.cs                 # Hello 构造/解析、ECDH、HKDF、Confirm MAC、transcript 链
│  │  │  └─ SessionCrypto.cs             # AesGcmBox.Seal/Open、SessionKeys（指纹/tiebreak/双向密钥）
│  │  ├─ Protocol/
│  │  │  ├─ Frame.cs                     # 帧读写 + FrameType 枚举（1-8）
│  │  │  ├─ Payloads.cs                  # ChatMessage/ProfileMessage JSON（camelCase）
│  │  │  └─ RoomCode.cs                 # 房间码编解码 + PSK 生成 + 本机 IP 枚举
│  │  └─ Transport/
│  │     └─ TcpChannel.cs                # AcceptAsync（ReuseAddress）/ ConnectAsync
│  ├─ SecureChat.App/                    # WinUI 3 非打包应用
│  │  ├─ App.xaml / App.xaml.cs          # 启动时 AppNotifier.Initialize() + TrayIcon.Initialize()
│  │  ├─ MainWindow.xaml / .cs           # 三区布局、ChatTemplateSelector 气泡模板、前后台跟踪、
│  │  │                                  #   未读计数、任务栏闪烁、AppWindow.Closing 拦截叉掉→托盘
│  │  ├─ ViewModels/
│  │  │  ├─ MainViewModel.cs             # CommunityToolkit.Mvvm：命令/状态/事件 DispatcherQueue 封送
│  │  │  └─ ChatMessage.cs               # 显示模型（方向/系统消息/时间）
│  │  ├─ Services/
│  │  │  ├─ AppNotifier.cs              # Toast 通知（不含内容），点击恢复窗口，静默降级
│  │  │  └─ TrayIcon.cs                # 系统托盘：Shell_NotifyIcon，双击显示/右键退出
│  │  └─ app.manifest                    # PerMonitorV2 DPI
└─ tests/
   └─ SecureChat.Core.Tests/             # xunit 2.9.2，回环 TCP 真实连接对（TestPair 辅助）
      ├─ SecureChatEndToEndTests.cs      # 握手/轮换/防重放端到端（9 用例）
      └─ RoomCodeTests.cs               # 房间码编解码/篡改/容错/IP 枚举（10 用例）
```

## 验证结果

1. **构建**：`dotnet build` 全解决方案 0 警告 0 错误。
2. **单元/端到端测试 19/19 全绿**：
   - **加密链路 9 用例**（回环 TCP 双真实实例）：握手后双方互发消息明文一致；姓名仅经加密通道交换；PSK 口令不匹配握手必败；手动轮换纪元 +1 且指纹变更、消息继续可达；双方同时轮换（碰撞）收敛；密文篡改 1 字节 GCM 认证失败；AAD 绑定（错误 epoch/type 解密必败）；不同纪元派生不同密钥；对端正常断开触发 Disconnected。
   - **房间码 10 用例**：编解码 roundtrip 字段一致（口令文本 43 字符）；PSK 256 位随机且不重复；空串/无前缀/非法 Base64/载荷过短等非法输入拒绝；截断与追加垃圾拒绝；前后空白与小写前缀容忍；本机 IP 枚举非空、格式合法、无重复。
3. **手工验证**（按需执行）：本机回环两实例互发；Wireshark 抓端口全为密文二进制；改口令重连被拒；缩短轮换周期观察纪元递增会话不断。

## 假设与决策记录

| 决策点 | 结论 | 理由 |
|---|---|---|
| 连接方式 | TCP 直连，一端监听一端连入，连接后全双工互发互收 | 固定两机场景最简可靠，全双工天然满足"互发互收" |
| 加密方案 | ECDH P-256 临时密钥 + PSK 认证 + AES-256-GCM + 周期轮换 | 前向保密 + 防中间人 |
| 历史记录 | 不落盘 | 聊天记录零留存，关闭即消失 |
| 姓名保护 | 仅在握手完成后经加密 Profile 帧传输 | 抓包全程不可见 |
| 抗流量分析 | 不做定长填充 | 内容机密性已由 GCM 保证；协议留有扩展位 |
| 部署形态 | 非打包 self-contained x64 | 免 MSIX 分发，目标机器免装运行时 |
| 轮换周期上限 | 1–720 分钟（计划时为 1–120）[实现修订] | 覆盖长会话低频轮换需求，720 上限防误配置 |
| Rekey 角色 [实现新增] | tiebreak 较低方为唯一 driver；较高方只能发 RekeyRequest | 消除双方同时轮换的竞态与死锁，碰撞自然收敛 |
| 新纪元确认 [实现新增] | driver 切换后发 Ping 作回执，高方等新纪元首帧 | 解决"调用方返回早于对端切换完成"的时序问题；Ping 也推进防重放计数器 |
| 在途帧 [实现新增] | 切换瞬间保留上一纪元接收密钥一轮 | 避免轮换边界处消息丢失 |
| 通知策略 [实现新增] | 仅窗口不在前台时提醒；Toast 只显示发送者名，绝不显示消息内容 | 知道来消息但不暴露内容；四路提醒互为兜底 |
| 窗口生命周期 [实现新增] | 叉掉 = 最小化到托盘后台驻留；托盘右键"退出"才真正结束 | 叉掉即退出会使后台通知失效；对齐常见 IM 惯例 |
| 房间码 [实现新增] | IP+端口+周期+口令打包为 `SC1-` 码，创建方生成/加入方粘贴；PSK 由 256 位 CSPRNG 自动生成 | 简化连接信息传递；顺带消除人工弱口令风险；信任边界 = 传房间码的渠道 |

## 使用说明

1. 拷贝整个 `win-x64` 发布目录到两台机器，运行 `SecureChat.exe`。
2. **创建房间**（一方）：填显示名 → 选择本机对外 IP（多网卡时注意选对）→ 端口/换钥周期按需调整 → 点"创建房间" → **复制房间码**，通过你信任的渠道（当面/电话）发给对方 → 等待加入。
3. **加入房间**（另一方）：填显示名 → 粘贴房间码 → 点"加入房间"。
4. 房间码内含 256 位机器生成的随机口令，无需人工设置/记忆口令；码即口令，**不要经由不信任的渠道传输**。
5. 界面密钥状态条：当前纪元、密钥指纹（双方一致即线路未被替换）、距下次换钥倒计时；可随时"立即换钥"。
6. 叉掉窗口 = 最小化到托盘继续收消息；托盘双击恢复，右键"退出"才真正结束。
