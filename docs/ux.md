# SecureChat — UX 设计

## 概述

SecureChat 桌面端界面由以下互斥视图构成，全部基于 React + TypeScript + Tailwind（设计令牌见 `src/ui/theme.css`）：

1. **显示名引导（NameSetup）**：首次启动（localStorage 未保存过名称）时覆盖全屏，设置一次后记住。
2. **连接面板（ConnectionPanel）**：创建/加入房间二选一，展示生成的房间码与连接状态。
3. **聊天视图（App）**：头部、消息列表、输入区与密钥状态 footer 分区布局。

### 关键交互约定

- **消息分组**：同一发送者的相邻消息在短时间内合并为一组，仅在组首显示“发送者 · 时间”；系统消息与 OVER 卡片打断分组。
- **消息回应**：对方消息右键（或 Shift+F10）在气泡正上方左对齐弹出回应面板（👍 / ❤️ / 📻 COPY THAT），固定位置不自动翻转；完成回应、点击外部或按 Esc 关闭，右键其他消息时切换面板。COPY THAT 同时复制原文并在气泡上盖“COPY THAT ✓”印章，双方可见，短暂显示后消失。
- **OVER 协商**：请求方显示“OVER 已发出…（撤回）”，对端弹出确认卡片，双方同意才挂断。
- **时间戳**：组内消息 hover 时在气泡侧边浮现。
- **安全可见性**：footer 常驻密钥纪元、指纹与换钥倒计时，支持手动立即换钥。

## 动效设计语言

界面动效统一使用 [motion](https://motion.dev) 库（`motion/react`），整体风格为**轻盈灵动**：低阻尼 spring 带来一次可感知的小回弹，动作短促清晰，但不反复摆动，不打扰聊天阅读。禁止引入 `framer-motion` 旧包，禁止在组件内自定 spring 参数造成手感漂移。

### 共享预设

所有动效参数收敛在 `src/ui/motion.ts`，组件只引用预设：

| 预设 | 用途 | 设计语言 |
| --- | --- | --- |
| `springEnter` | 气泡、卡片、视图等入场 | 带方向感的轻盈弹入与小幅回弹 |
| `springPop` | 回应 chips、数字、悬浮面板等小元素 | 弹出干脆，强调确认感，避免连续弹跳 |
| `emojiPulse` | 回应表情 | 表情单独放大后回落，pill 外框保持稳定；多个表情按顺序响应 |
| `statusRipple` | 已连接状态灯 | 波纹柔和向外扩散，传达持续连接 |
| `breathPulse` | 等待状态灯、OVER 等待提示 | 缓慢呼吸，表达等待而非催促 |

### 无障碍

- `src/main.tsx` 根部以 `<MotionConfig reducedMotion="user">` 包裹：系统开启“减少动态效果”时自动关闭 transform/layout 动画，仅保留 opacity 与颜色过渡。
- 纯 CSS 过渡（如 hover 时间戳）沿用 `motion-safe:transition-*` 前缀；`theme.css` 的 `prefers-reduced-motion` 兜底块继续保留。
- CSS 中不再维护 keyframes 动画；`theme.css` 原有的气泡、卡片、印章与等待脉冲动画已迁移至 motion。
- 动画关闭或被打断时仍应落到可阅读的最终状态，不隐藏内容，不影响焦点与文本选择。

### 动效点清单

| 场景 | 位置 | 设计语言 |
| --- | --- | --- |
| 消息气泡入场 | `MessageList` | 从发送方向的一侧轻盈滑入并小幅回弹 |
| 新消息滚动跟随 | `MessageList` | 贴底时平滑吸附，用户翻阅历史时不打断 |
| 回应 chips | `MessageList` | 外框弹入并平滑重排；表情依次单次放大回落，计数变化重新强调 |
| 回应悬浮面板 | `MessageList` | 以左上角为原点缩放进出，动作轻快，不改变消息布局 |
| COPY THAT 印章 | `MessageList` | 砸下、微旋转回弹、短暂停留后淡出 |
| OVER 确认卡片 | `MessageList` | 上浮并缩放进场，突出需要决策的内容 |
| 顶部状态灯 | `App` | 已连接时波纹扩散，等待时呼吸脉冲 |
| OVER 已发出 | `App` | 文案呼吸脉冲，持续传达等待状态 |
| 复制房间码 | `App` | 复制结果与原交叉滑动淡入，提供明确反馈 |
| 视图切换 | `NameSetup` / `ConnectionPanel` / `App` | 遮罩淡入、卡片 spring 弹入，聊天主视图整体上浮 |

### 扩展约定

新增动效时优先复用共享预设；确需新参数时在 `src/ui/motion.ts` 中新增预设并更新本节。列表、面板与开关类元素需要退出动画时使用 `AnimatePresence` 并保证稳定的 `key`。表情跳动只作用于表情本身，不让 pill 外框或消息布局跟随晃动。
