fn main() {
    println!("cargo::rerun-if-changed=src/exports.def");
    if let Ok(os) = std::env::var("CARGO_CFG_TARGET_OS")
        && os == "windows"
    {
        println!("cargo::rustc-link-arg-cdylib=/DEF:src\\exports.def");
    }
}
