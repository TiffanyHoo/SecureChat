# 启动 SecureChat 双实例（tauri dev 模式，前端走 vite 热更新，无需 release 产物）：
#   vite   —— 全局唯一一份（1428 空闲时拉起，否则复用，常驻独立控制台窗口）
#   实例 A —— 创建房间
#   实例 B —— 加入房间
# 用法：powershell -ExecutionPolicy Bypass -File scripts\start-dual.ps1
# 说明：
#   - 两个实例用 --config 把 beforeDevCommand 置空（scripts\tauri.no-dev-server.json），
#     不再各自拉 vite；tauri CLI 会自己轮询 devUrl 直到 vite 就绪才启动应用，无竞态；
#   - 两个实例使用独立 WebView2 用户数据目录（%TEMP%\SecureChat-A / SecureChat-B），
#     避免窗口起不来；
#   - 每个实例一个控制台窗口展示 tauri/cargo 日志，关闭窗口或 Ctrl+C 即停对应实例；
#   - vite 常驻后台，重跑脚本秒复用；想彻底清理需关闭 vite 那个控制台窗口。
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot\..

$instanceArgs = 'tauri', 'dev', '--no-watch', '--config', "$PSScriptRoot\tauri.no-dev-server.json"

# vite：全局唯一一份 dev server
if (-not (Get-NetTCPConnection -LocalPort 1428 -State Listen -ErrorAction SilentlyContinue)) {
    Start-Process pnpm.cmd -ArgumentList 'dev'
    Write-Host '已拉起 vite dev server（1428），两个实例会等它就绪后自动启动'
}
else {
    Write-Host '检测到 1428 端口已有 vite dev server，直接复用'
}

# 实例 A（创建房间）
$env:WEBVIEW2_USER_DATA_FOLDER = "$env:TEMP\SecureChat-A"
Start-Process pnpm.cmd -ArgumentList $instanceArgs

# 实例 B（加入房间）
$env:WEBVIEW2_USER_DATA_FOLDER = "$env:TEMP\SecureChat-B"
Start-Process pnpm.cmd -ArgumentList $instanceArgs

Write-Host '已启动两个 SecureChat 实例（A：创建房间 / B：加入房间）'
