use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=swift/ScreenCaptureBridge.swift");

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("macos") {
        return;
    }

    build_swift_bridge();
}

fn build_swift_bridge() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let swift_source = Path::new("swift/ScreenCaptureBridge.swift");
    let object_file = out_dir.join("ScreenCaptureBridge.o");
    let library_file = out_dir.join("libscreen_capture_bridge.a");

    let sdk_path = command_output("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let swiftc_path = command_output("xcrun", &["--find", "swiftc"]);
    let libtool_path = command_output("xcrun", &["--find", "libtool"]);

    // 1. Compile Swift to Object file
    let swiftc_status = Command::new(&swiftc_path)
        .arg("-parse-as-library")
        .arg("-O")
        .arg("-emit-object")
        .arg(swift_source)
        .arg("-o")
        .arg(&object_file)
        .arg("-sdk")
        .arg(&sdk_path)
        // Add specific target if needed, e.g., .arg("-target").arg("arm64-apple-macosx12.0")
        .status()
        .expect("failed to run swiftc");

    if !swiftc_status.success() {
        panic!("swiftc failed");
    }

    // 2. Package into static lib
    let libtool_status = Command::new(&libtool_path)
        .arg("-static")
        .arg("-o")
        .arg(&library_file)
        .arg(&object_file)
        .status()
        .expect("failed to run libtool");

    if !libtool_status.success() {
        panic!("libtool failed");
    }

    // 3. Link instructions for Cargo
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=screen_capture_bridge");

    // 1. Tell Cargo where to find the Swift standard libraries at link-time
    // This path is safe and won't cause the "duplicate" error because
    // it's the official system location.
    println!("cargo:rustc-link-search=native=/usr/lib/swift");

    // 2. Add the rpath so the executable knows where to look at runtime
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    // 3. Link the necessary Swift libraries
    println!("cargo:rustc-link-lib=swiftCore");
    println!("cargo:rustc-link-lib=swift_Concurrency");

    // Frameworks
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=IOSurface");
}

fn infer_swift_runtime_dir(swift_link_dir: &Path) -> PathBuf {
    let system_runtime_dir = PathBuf::from("/usr/lib/swift");
    if system_runtime_dir
        .join("libswift_Concurrency.dylib")
        .exists()
    {
        return system_runtime_dir;
    }

    swift_link_dir.to_path_buf()
}

fn infer_swift_link_dir(swiftc_path: &str) -> PathBuf {
    let swiftc = Path::new(swiftc_path);
    let toolchain_dir = swiftc
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("unable to locate the Swift toolchain directory");
    let base_dir = toolchain_dir.join("usr/lib");
    let preferred_dir = base_dir.join("swift/macosx");

    if preferred_dir.join("libswift_Concurrency.dylib").exists() {
        return preferred_dir;
    }

    let compatibility_dir = base_dir.join("swift-5.5/macosx");
    if compatibility_dir
        .join("libswift_Concurrency.dylib")
        .exists()
    {
        return compatibility_dir;
    }

    preferred_dir
}

fn command_output(command: &str, args: &[&str]) -> String {
    let output = Command::new(command)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {}", command, e));

    if !output.status.success() {
        panic!(
            "{} {:?} failed: {}",
            command,
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout)
        .expect("command output was not valid UTF-8")
        .trim()
        .to_string()
}
