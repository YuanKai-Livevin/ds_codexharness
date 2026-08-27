# run_dev.ps1 — 以开发模式运行应用（OH_DEV_ROOT 指向项目根，便于定位 runtime/codex-bin）
$root = Split-Path $PSScriptRoot -Parent
$env:OH_DEV_ROOT = $root
& "$root\target\debug\office-harness.exe"
