fn main() {
    // 关键：tauri-build 不会自动跟踪 frontendDist 目录的变化，
    // 若不声明，前端资源改动不会触发重新编译，旧 UI 会一直被嵌进 exe。
    // 显式声明 assets 目录与配置文件，任何前端改动都会触发完整重编译。
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
