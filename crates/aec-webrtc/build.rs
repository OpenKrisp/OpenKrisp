use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest"));
    let root = manifest.join("../..");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output"));
    let host = env::var("HOST").expect("Cargo host");
    let target = env::var("TARGET").expect("Cargo target");
    assert_eq!(
        host, target,
        "AEC prototype requires a native build (use a Linux container for Linux)"
    );
    assert!(
        target.contains("apple-darwin") || target.contains("linux-gnu"),
        "unsupported AEC platform"
    );
    for path in [
        root.join("scripts/build-aec.py"),
        root.join("third_party/aec"),
        manifest.join("src/bridge.cc"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for key in [
        "OPENKRISP_AEC_TOOLS",
        "CC",
        "CXX",
        "MACOSX_DEPLOYMENT_TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let output = Command::new("python3")
        .arg(root.join("scripts/build-aec.py"))
        .arg(&out)
        .arg(env::var("NUM_JOBS").unwrap_or_else(|_| "2".into()))
        .output()
        .expect("Python 3 is required to build native AEC");
    std::fs::write(
        out.join("native-build.log"),
        [&output.stdout[..], &output.stderr[..]].concat(),
    )
    .expect("write native build log");
    assert!(
        output.status.success(),
        "native AEC build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.join("libopenkrisp_aec_native.a").is_file(),
        "native build did not produce archive"
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=openkrisp_aec_native");
    if target.contains("apple-darwin") {
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-link-lib=framework=Foundation");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=pthread");
    }
}
