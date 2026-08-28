//! 模型供应商配置生成：CODEX_HOME 下的 config.toml 与审批规则（T0-04 安全生成）。

use crate::config::AppSettings;
use std::path::Path;

/// 生成 CODEX_HOME 下的 config.toml 与审批规则。
/// `bridge_port`：启用内置翻译层时本地网关端口（None 则直连 settings.base_url）。
pub fn prepare_home(codex_home: &Path, settings: &AppSettings, bridge_port: Option<u16>) -> Result<(), String> {
    std::fs::create_dir_all(codex_home.join("rules")).map_err(|e| e.to_string())?;
    // 内网免密钥模式：requires_openai_auth=false，引擎不校验 Key
    let no_auth_line = if settings.no_auth {
        "requires_openai_auth = false\n"
    } else {
        ""
    };
    // 启用内置翻译层时，引擎 base_url 指向本地网关（/responses → /chat/completions）
    let engine_base = match (settings.use_bridge, bridge_port) {
        (true, Some(port)) => format!("http://127.0.0.1:{}/", port),
        (true, None) => format!("http://127.0.0.1:{}/", crate::MEMORY_PORT), // 兜底（不应发生）
        (false, _) => settings.base_url.clone(),
    };
    // 安全生成（T0-04）：provider 内部 ID 固定为安全 slug；base_url/model/env 做 TOML 转义，
    // 避免引号/换行/Unicode 破坏 config.toml
    let provider_slug = safe_slug(&settings.provider_name);
    let model = toml_escape(&settings.model);
    let base_url = toml_escape(&engine_base);
    let env_key = toml_escape(&settings.api_key_env);
    let cfg = format!(
        r#"model = "{}"
model_provider = "{}"
approval_policy = "on-request"
sandbox_mode = "{}"

[windows]
sandbox = "{}"

[model_providers.{}]
name = "{}"
base_url = "{}"
wire_api = "responses"
env_key = "{}"
{}"#,
        model,
        provider_slug,
        settings.sandbox_mode,
        if settings.windows_sandbox == "elevated" { "elevated" } else { "unelevated" },
        provider_slug,
        toml_escape(&settings.provider_name),
        base_url,
        env_key,
        no_auth_line,
    );
    std::fs::write(codex_home.join("config.toml"), cfg).map_err(|e| e.to_string())?;

    let policy = r#"# 办公助手审批规则：破坏性操作一律要求人工确认
prefix_rule(pattern=["rm"], decision="prompt")
prefix_rule(pattern=["rmdir"], decision="prompt")
prefix_rule(pattern=["del"], decision="prompt")
prefix_rule(pattern=["erase"], decision="prompt")
prefix_rule(pattern=["rd"], decision="prompt")
prefix_rule(pattern=["Remove-Item"], decision="prompt")
prefix_rule(pattern=["format"], decision="prompt")
prefix_rule(pattern=["pip"], decision="prompt")
prefix_rule(pattern=["pip3"], decision="prompt")
prefix_rule(pattern=["move"], decision="prompt")
prefix_rule(pattern=["ren"], decision="prompt")
prefix_rule(pattern=["rename"], decision="prompt")
prefix_rule(pattern=["mv"], decision="prompt")
prefix_rule(pattern=["git", "reset"], decision="prompt")
prefix_rule(pattern=["git", "clean"], decision="prompt")
"#;
    std::fs::write(codex_home.join("rules").join("office.policy"), policy)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// TOML 基本字符串转义（防引号/反斜杠破坏配置）。
pub(crate) fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 安全 provider 内部 ID：仅保留 [A-Za-z0-9_-]，非法或为空时回退 "custom"。
pub(crate) fn safe_slug(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        "custom".to_string()
    } else {
        cleaned
    }
}
