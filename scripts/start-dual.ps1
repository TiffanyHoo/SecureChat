# 启动 SecureChat 双实例（tauri dev 模式，前端走 vite 热更新，无需 release 产物）：
#   实例 A —— 创建房间（1428 空闲时由它拉起 vite dev server，被占用则直接复用）
#   实例 B —— 加入房间（--no-dev-server 复用同一 dev server）
# 用法：powershell -ExecutionPolicy Bypass -File scripts\start-dual.ps1
# 说明：
#   - 两个实例使用独立 WebView2 用户数据目录（%TEMP%\SecureChat-A / SecureChat-B），避免窗口起不来；
#   - 实例 A 带文件监听（改代码自动重启），实例 B 不监听、保持稳定；
#   - 每个实例会开一个控制台窗口展示 tauri/vite 日志，关闭控制台或 Ctrl+C 即停对应实例。
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot\..

# 实例 A（创建房间）
$env:WEBVIEW2_USER_DATA_FOLDER = "$env:TEMP\SecureChat-A"
if (Get-NetTCPConnection -LocalPort 1428 -State Listen -ErrorAction SilentlyContinue) {
    Write-Host '检测到 1428 端口已有 dev server，直接复用'
    Start-Process pnpm.cmd -ArgumentList 'tauri', 'dev', '--no-dev-server', '--no-watch'
}
else {
    Start-Process pnpm.cmd -ArgumentList 'tauri', 'dev'
    # 等待 vite 就绪后再启动实例 B（编译期间 vite 早已起来）
    for ($i = 0; $i -lt 90; $i++) {
        if (Get-NetTCPConnection -LocalPort 1428 -State Listen -ErrorAction SilentlyContinue) { break }
        Start-Sleep -Seconds 1
    }
}

Start-Sleep -Seconds 1

# 实例 B（加入房间）
$env:WEBVIEW2_USER_DATA_FOLDER = "$env:TEMP\SecureChat-B"
Start-Process pnpm.cmd -ArgumentList 'tauri', 'dev', '--no-dev-server', '--no-watch'

Write-Host '已启动两个 SecureChat 实例（A：创建房间 / B：加入房间）'
