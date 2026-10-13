//! 빌드 스크립트. 아이콘을 바꾸면 실행 파일도 다시 빌드되어야 한다 —
//! tauri-build 는 아이콘 파일을 재빌드 조건으로 알려 주지 않으므로 직접 적는다.

fn main() {
    for icon in [
        "icons/icon.ico",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }
    tauri_build::build()
}
