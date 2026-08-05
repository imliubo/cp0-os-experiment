fn main() {
    println!("cargo:rerun-if-changed=src/v4l2_jpeg.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        cc::Build::new()
            .file("src/v4l2_jpeg.c")
            .flag_if_supported("-Wall")
            .flag_if_supported("-Wextra")
            .flag_if_supported("-Werror")
            .compile("cp0_v4l2_jpeg");
    }
}
