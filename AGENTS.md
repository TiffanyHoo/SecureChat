# AGENTS

## 版本号规范

- 项目中多处记录版本号，唯一权威来源是 `src-tauri/tauri.conf.json` 中的 `version` 字段；客户端打包也使用该版本号。其他位置的版本号一律以此为准，修改版本时只改这里（或由此同步）。

## 提交规范

- Commit message 使用中文并遵循 Conventional Commits，格式为 `<type>(<scope>): <中文描述>`；`scope` 可省略，单号可放在描述末尾，如 `(issue9)`。
- 类型仅使用 `feat`、`fix`、`docs`、`style`、`refactor`、`perf`、`test`、`build`、`ci`、`chore` 或 `revert`。
- Header 最长 200 字符，正文与标题之间保留空行；涉及多个文件时概括关键变更，不逐文件列举。
- 提交前运行 `pnpm test` 和 `pnpm build`，不添加 `Co-Authored-By`。
