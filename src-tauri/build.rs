fn main() {
    // 防护:tauri 的前端资源必须在编译期嵌入(dist 目录),否则 WebView2 会回退到
    // devUrl(localhost:1420) 显示 ERR_CONNECTION_REFUSED。
    // 裸 `cargo build` 若 dist 缺失/过旧会产出缺资源的坏 exe,这里直接编译失败提示。
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let dist_index = manifest_dir.join("../dist/index.html");
    if !dist_index.exists() {
        panic!(
            "前端资源缺失: 未找到 {}。\n\
             请先运行 `pnpm build`(或 `pnpm tauri build`,它会自动构建前端),\n\
             然后使用 `pnpm tauri build` 构建,不要用裸 `cargo build`。",
            dist_index.display()
        );
    }
    println!("cargo:rerun-if-changed=../dist/index.html");
    println!("cargo:rerun-if-changed=../dist/assets");
    tauri_build::build()
}
