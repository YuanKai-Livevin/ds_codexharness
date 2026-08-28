# build.ps1 - Assemble the distributable OfficeHarness folder (versioned output).
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\build.ps1
$root = Split-Path $PSScriptRoot -Parent
# Versioned output dir so a running old build never blocks the copy.
$dist = Join-Path $root "dist\OfficeHarness-v0.3"

function Fail($msg) { Write-Output "ERROR: $msg"; exit 1 }

Write-Output "== [1/4] Check prerequisites =="
if (-not (Test-Path "$root\runtime\python312\python.exe")) {
    Write-Output "Python runtime missing; run scripts\setup_python.ps1 first."
    & "$PSScriptRoot\setup_python.ps1"
}
if (-not (Test-Path "$root\vendor\codex-bin\codex-app-server.exe")) {
    Fail "codex binaries missing; run scripts\fetch_codex_bins.py first."
}

Write-Output "== [2/4] cargo build --release =="
Push-Location $root
# 强制重编译 app crate：Tauri 前端资源在宏展开时嵌入，cargo 指纹不跟踪 assets，
# 增量构建会嵌入旧前端。cargo clean -p 只清 app，保留依赖缓存，开销小。
cargo clean -p office-harness 2>$null
cargo build --release --workspace 2>$null
if ($LASTEXITCODE -ne 0) { Pop-Location; Fail "cargo build failed (exit $LASTEXITCODE)" }
Pop-Location

Write-Output "== [3/4] Assemble dist =="
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force -Path $dist | Out-Null | Out-Null

# Copy exe and verify size (catch silent copy failure when old process locks the file)
$srcExe = "$root\target\release\office-harness.exe"
$dstExe = Join-Path $dist "OfficeHarness.exe"
Copy-Item $srcExe $dstExe -Force -ErrorAction Stop
if ((Get-Item $srcExe).Length -ne (Get-Item $dstExe).Length) {
    Fail "exe copy check failed: please close the running app and rebuild"
}
Write-Output ("  exe copied (" + (Get-Item $dstExe).Length + " bytes)")

Copy-Item -Recurse "$root\runtime\python312" (Join-Path $dist "runtime\python312") -Force
Write-Output "  python runtime copied"

New-Item -ItemType Directory -Force -Path (Join-Path $dist "codex-bin") | Out-Null
foreach ($n in @("codex.exe", "codex-app-server.exe", "codex-command-runner.exe", "codex-windows-sandbox-setup.exe")) {
    Copy-Item (Join-Path "$root\vendor\codex-bin" $n) (Join-Path $dist "codex-bin") -Force
}
Write-Output "  codex binaries copied"

# 记忆面板功能块（backend + frontend + 离线编码 + 依赖清单）
New-Item -ItemType Directory -Force -Path (Join-Path $dist "memory-block") | Out-Null
Copy-Item -Recurse "$root\backend" (Join-Path $dist "memory-block\backend") -Force
Copy-Item -Recurse "$root\frontend" (Join-Path $dist "memory-block\frontend") -Force
# 剔除 Python 缓存目录，保持发行包干净
Get-ChildItem (Join-Path $dist "memory-block") -Recurse -Directory -Filter "__pycache__" | Remove-Item -Recurse -Force
Write-Output "  memory block copied"

# 确定性办公工具包（R10 office-tools：otools.py + otools_lib + SKILL.md）
Copy-Item -Recurse "$root\vendor\office-tools" (Join-Path $dist "office-tools") -Force
Get-ChildItem (Join-Path $dist "office-tools") -Recurse -Directory -Filter "__pycache__" | Remove-Item -Recurse -Force
Write-Output "  office-tools copied"

Write-Output "== [4/4] Summary =="
$size = (Get-ChildItem $dist -Recurse -File | Measure-Object -Property Length -Sum).Sum / 1MB
Write-Output ("dist\OfficeHarness ready, {0:N1} MB" -f $size)
Get-ChildItem $dist | Select-Object Name

$zip = Join-Path $root "dist\OfficeHarness-v0.3.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $dist -DestinationPath $zip -CompressionLevel Optimal
Write-Output ("zipped: {0} ({1:N1} MB)" -f $zip, ((Get-Item $zip).Length / 1MB))
