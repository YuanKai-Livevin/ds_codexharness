@echo off
rem Join the split release archive parts into OfficeHarness-v0.3.zip
copy /b OfficeHarness-v0.3.zip.001+OfficeHarness-v0.3.zip.002+OfficeHarness-v0.3.zip.003+OfficeHarness-v0.3.zip.004 OfficeHarness-v0.3.zip >nul
if exist OfficeHarness-v0.3.zip (
    echo OK: OfficeHarness-v0.3.zip created. Unzip and run OfficeHarness.exe
) else (
    echo FAIL: please make sure all 4 parts are in this folder.
)
pause
