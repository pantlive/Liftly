#[path = "build/rsrc.rs"]
mod rsrc;

use std::env;
use std::path::PathBuf;

fn main() {
    for path in ["build.rs", "build/rsrc.rs", "assets/liftly.ico"] {
        println!("cargo:rerun-if-changed={path}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // 使用目标架构而非构建主机架构，兼容本机和交叉编译。
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("缺少目标架构");
    let (machine, reloc_type) = match arch.as_str() {
        "x86_64" => (0x8664, 3),
        "aarch64" => (0xaa64, 2),
        "x86" => (0x014c, 7),
        _ => {
            println!("cargo:warning=目标架构 {arch} 暂不支持嵌入 Liftly 图标");
            return;
        }
    };
    let frames = rsrc::parse_ico(include_bytes!("assets/liftly.ico")).expect("Liftly ICO 格式错误");
    let object = rsrc::build_rsrc_coff(&frames, machine, reloc_type);
    let path =
        PathBuf::from(env::var_os("OUT_DIR").expect("缺少构建输出目录")).join("liftly_rsrc.o");
    std::fs::write(&path, object).expect("写入图标资源目标文件失败");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}
