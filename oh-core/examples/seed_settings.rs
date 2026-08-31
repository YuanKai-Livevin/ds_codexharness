//! 预置设置：写入一份含 DPAPI 加密 API Key 的 settings.json，用于验证自动启动。
//! 运行: cargo run --example seed_settings -- <workspace> <api_key>
use oh_core::config::AppSettings;
use oh_core::dpapi;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let workspace = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "F:\\dshProject\\codexharness\\demo-workspace".into());
    let key = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "sk-fake-auto-start-key".into());

    let root = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OfficeHarness");
    let settings_path = root.join("settings.json");

    let enc = dpapi::encrypt(&key).expect("encrypt");
    let settings = AppSettings {
        workspace_path: workspace,
        api_key_enc: Some(enc),
        onboarded: true,
        ..Default::default()
    };
    settings.save(&settings_path).expect("save");
    println!("settings written to {}", settings_path.display());
    println!(
        "api_key_enc length: {}",
        settings
            .api_key_enc
            .as_deref()
            .map(|s| s.len())
            .unwrap_or(0)
    );
    // 回读验证
    let loaded = AppSettings::load(&settings_path);
    let dec = dpapi::decrypt(loaded.api_key_enc.as_deref().unwrap()).expect("decrypt");
    println!("roundtrip key ok: {}", dec == key);
}
