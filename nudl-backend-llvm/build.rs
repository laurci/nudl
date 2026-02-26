fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let obj_path = format!("{}/nudl_rt.o", out_dir);
    let status = std::process::Command::new("cc")
        .args(["-c", "-O2", "-o", &obj_path, "../runtime/nudl_rt.c"])
        .status()
        .expect("failed to compile nudl runtime (is cc available?)");
    assert!(status.success(), "nudl runtime compilation failed");
    println!("cargo:rerun-if-changed=../runtime/nudl_rt.c");
    println!("cargo:rerun-if-changed=../runtime/nudl_rt.h");
}
