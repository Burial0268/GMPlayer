fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    }
}
