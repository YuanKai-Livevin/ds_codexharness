# JONHON Harness

A rigorous local office-automation desktop tool for Windows: wraps the OpenAI Codex engine with a bundled Python 3.12 runtime, and confines the AI assistant strictly to a user-selected **workspace** for office tasks (Excel / Word / PPT / PDF / image batch processing).

- Version: v0.3.0
- Platform: Windows 10/11 (64-bit)
- Stack: Rust (Tauri 2) + vanilla HTML/CSS/JS, Python 3.12

## Highlights

- **Workspace sandbox** — all file operations are confined to the workspace; path escapes (`../`, drive roots, system dirs) are rejected before execution; destructive operations require explicit user approval.
- **Session/task recovery** — history restores text, tool calls (command + output, collapsible), file changes and per-turn status (completed / failed / interrupted).
- **Memory panel** — SQLite-backed memory blocks, context watermark (52K warn / 60K red line), smart compaction, phase summaries with explicit hand-off messages into new sessions.
- **Tasks & artifacts view** — every turn becomes a task card (goal / status / tokens / duration / approvals); artifact cards support open, reveal, and line-level before/after **Diff**.
- **Structured audit** — SQLite audit log records goals, model/gateway, workspace, approvals, tool calls, file changes, errors, tokens/cost and final acceptance; auto-redacted; diagnostics exported only on user request.
- **Deterministic office tool runtime** — the `office-tools` skill ships 13 stable tools (Excel merge/dedupe/filter/pivot/formula check, Word template fill, PDF merge/split/text, image resize/convert, file manifest/rename) with parameter schemas, permission bounds, `--dry-run`, output validation, typed error codes and a built-in test suite (20 checks).
- **SKILLS governance** — metadata (version / author / permissions / checksum), enable/disable, import with auto-backup and rollback, built-in test tasks.
- **Intranet deployment** — optional keyless mode (`requires_openai_auth=false`) and a built-in translation layer (/responses → /chat/completions, streaming + reasoning + tool calls) for gateways that only support chat completions. A "Test connection" probe auto-detects gateway capabilities.

## Layout

- `app/` — Tauri shell (commands: settings / workspace / engine / sessions / skills / memory / audit / tasks / office)
- `oh-core/` — core library (Codex JSON-RPC client, config, DPAPI encryption, workspace validation, prompts)
- `backend/` + `frontend/` — memory service (FastAPI, token-authenticated) and its panel UI
- `tools/office-tools/` — deterministic office tool package (versioned)
- `scripts/` — build & ops scripts (`build.ps1`, `setup_python.ps1`)
- `release/` — split archive parts (GitHub 100 MB limit); download all parts and run `合并.bat` to reassemble

## Build

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup_python.ps1   # bundled Python 3.12 + office libs
python scripts\fetch_codex_bins.py                                  # codex engine binaries
powershell -ExecutionPolicy Bypass -File scripts\build.ps1          # release build + assemble dist
```

Output: `dist\OfficeHarness-v0.3\` + `OfficeHarness-v0.3.zip` (~280 MB). Note: `build.ps1` must be saved as **UTF-8 with BOM** (Windows PowerShell 5.1 misreads BOM-less UTF-8 Chinese comments as ANSI and breaks parsing).

## Data

App-level data lives under `C:\HARNESS\` (settings.json, codex-home, skills, disabled-skills, skill-backups, audit\audit.db, logs). Workspace-scoped data lives in `{workspace}\.harness-memory\` and `.oh_tmp\`.

See `README.md` (Chinese) for full details.
