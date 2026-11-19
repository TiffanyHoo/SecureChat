# SecureChat

WinUI 3 端到端加密聊天桌面应用：两台机器通过 TCP 直连秘密通话，线路全程密文（含姓名），界面显示解密后的明文。

## 特性

- **端到端加密**：ECDH P-256 临时密钥协商 + PSK 口令认证（防中间人）+ AES-256-GCM 消息加密；姓名与消息在网络上全程不可见，抓包无法还原任何内容
- **房间码一键入会**：创建房间自动生成 256 位随机口令，与 IP/端口打包为一段 `SC1-` 房间码；对方粘贴即连，无需手动传 IP 和口令
- **会话密钥自动轮换**：默认每 10 分钟（可设 1–720 分钟）通过新一轮 ECDH 换钥，各纪元密钥完全隔离，具备前向保密；界面实时显示纪元/密钥指纹/倒计时
- **防篡改防重放**：帧头经 AAD 绑定进 GCM 认证，nonce 计数器严格递增，篡改或重放任意一帧都会被拒绝
- **后台驻留提醒**：叉掉窗口最小化到系统托盘继续收消息；来消息弹系统 Toast（只提示来源、不含内容）+ 任务栏闪烁 + 未读计数 + 提示音，点通知即回窗口
- **零落盘**：不保存任何聊天记录，关闭即消失

## 构建与运行

要求：Windows 10 17763+ / .NET 8 SDK

```bash
git clone <repo-url>
cd SecureChat
dotnet build SecureChat.sln
dotnet run --project src/SecureChat.App
```

发布 self-contained 版本（目标机器免装 .NET）：

```bash
dotnet publish src/SecureChat.App -c Release
# 产物位于 src/SecureChat.App/bin/x64/Release/.../win-x64/publish，整目录拷走即可运行
```

## 使用方法

1. **创建房间**（一方）：填显示名 → 选择本机对外 IP → 点「创建房间」→ 点「复制房间码」，通过你信任的渠道（当面/电话）发给对方
2. **加入房间**（另一方）：填显示名 → 粘贴房间码 → 点「加入房间」
3. 聊天窗口显示明文，线路传输密文；底部状态条实时显示密钥纪元/指纹/换钥倒计时，可随时「立即换钥」

> 房间码内含会话口令，码即口令，务必经可信渠道传递。
> 点对点直连，跨公网使用需自行处理端口映射/防火墙放行。

## 测试

19 个测试覆盖完整加密链路（回环 TCP 真实双实例）与房间码编解码：

```bash
dotnet test tests/SecureChat.Core.Tests
```

## 项目结构

| 工程 | 说明 |
|---|---|
| `src/SecureChat.Core` | 协议核心：帧读写、ECDH 握手、AES-GCM 会话、密钥轮换、TCP 通道（net8.0 纯类库，无第三方依赖） |
| `src/SecureChat.App` | WinUI 3 界面：房间码流程、聊天气泡、托盘驻留、Toast 通知 |
| `tests/SecureChat.Core.Tests` | xunit：握手/轮换/防重放端到端 + 房间码单元测试 |

完整协议设计（握手流程、密钥派生、轮换机制、防重放、房间码格式）见 [docs/design.md](docs/design.md)。
