use std::{env, fs, path::Path};

use rsf::rust_codegen::AddrType;

fn main() {
    let code = rsf::rust_codegen::codegen(
        "../rsf/tf2.rsf".into(),
        AddrType::U32,
    )
    .unwrap();

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("tf2_rpi.rs");
    fs::write(&dest_path, code).unwrap();

    println!("cargo::rerun-if-changed=../rsf/tf2.rsf")
}
