# setup_python.ps1 — Build the bundled Python 3.12 runtime with office libraries.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$dl = Join-Path $root "vendor\python-dl"
$runtime = Join-Path $root "runtime"
$pyDir = Join-Path $runtime "python312"

Write-Output "== extracting python embeddable =="
if (-not (Test-Path (Join-Path $pyDir "python.exe"))) {
    New-Item -ItemType Directory -Force -Path $pyDir | Out-Null
    python -c "import zipfile; zipfile.ZipFile(r'$dl\python-embed.zip').extractall(r'$pyDir')"
    Write-Output "extracted to $pyDir"
} else {
    Write-Output "python already present, skip extract"
}

Write-Output "== enabling site-packages =="
$pth = Join-Path $pyDir "python312._pth"
$content = Get-Content $pth
$content = $content -replace '^#(import site)', 'import site'
if ($content -notcontains 'Lib\site-packages') {
    $content += 'Lib\site-packages'
}
Set-Content -Path $pth -Value $content -Encoding ASCII
Get-Content $pth

Write-Output "== bootstrapping pip =="
& (Join-Path $pyDir "python.exe") "$dl\get-pip.py" --no-warn-script-location 2>&1 | Select-Object -Last 3
if ($LASTEXITCODE -ne 0) { throw "get-pip failed" }

Write-Output "== installing office libraries =="
$libs = @(
    "openpyxl", "xlsxwriter", "python-docx", "python-pptx", "pypdf",
    "pandas", "numpy", "pillow", "xlrd", "matplotlib", "requests",
    "python-dateutil", "et_xmlfile"
)
$py = Join-Path $pyDir "python.exe"
$pipArgs = @("-m", "pip", "install", "--no-warn-script-location", "--timeout", "60", "--retries", "10") + $libs
& $py @pipArgs 2>&1 | Select-Object -Last 6
if ($LASTEXITCODE -ne 0) {
    Write-Output "== pypi failed, retry with TUNA mirror =="
    $mirror = @("-m", "pip", "install", "--no-warn-script-location", "--timeout", "60", "--retries", "10",
                "-i", "https://pypi.tuna.tsinghua.edu.cn/simple") + $libs
    & $py @mirror 2>&1 | Select-Object -Last 6
    if ($LASTEXITCODE -ne 0) { throw "pip install failed on both indexes" }
}

Write-Output "== verification =="
& $py -c "import openpyxl, pandas, docx, pptx, pypdf, PIL, xlsxwriter; print('office libs OK:', openpyxl.__version__, pandas.__version__)"
Write-Output "== done =="
