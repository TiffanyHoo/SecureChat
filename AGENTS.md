# AGENTS

## 提交规范

- Commit message 使用中文并遵循 Conventional Commits，格式为 `<type>(<scope>): <中文描述>`；`scope` 可省略，单号可放在描述末尾，如 `(issue9)`。
- 类型仅使用 `feat`、`fix`、`docs`、`style`、`refactor`、`perf`、`test`、`build`、`ci`、`chore` 或 `revert`。
- Header 最长 200 字符，正文与标题之间保留空行；涉及多个文件时概括关键变更，不逐文件列举。
- 提交前运行 `pnpm test` 和 `pnpm build`，不添加 `Co-Authored-By`。
