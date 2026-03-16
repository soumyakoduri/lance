// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

//! Build script for lance-io
//!
//! When the `rgw` feature is enabled, this script configures linking
//! against the Ceph RGW libraries.

fn main() {
    #[cfg(feature = "rgw")]
    {
        configure_rgw_linking();
    }
}

#[cfg(feature = "rgw")]
fn configure_rgw_linking() {
    use std::env;
    use std::path::PathBuf;

    // Get CEPH_PATH from environment or use default relative path
    let ceph_path = env::var("CEPH_PATH").unwrap_or_else(|_| {
        // Default to ../ceph relative to the workspace root
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let workspace_root = PathBuf::from(&manifest_dir)
            .parent() // rust
            .and_then(|p| p.parent()) // lance
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(&manifest_dir));

        workspace_root
            .parent() // lancedb-code
            .map(|p| p.join("ceph"))
            .unwrap_or_else(|| PathBuf::from("../ceph"))
            .to_string_lossy()
            .to_string()
    });

    let ceph_path = PathBuf::from(&ceph_path);
    let lib_path = ceph_path.join("build/lib");

    // Verify paths exist
    if !lib_path.exists() {
        panic!(
            "Ceph library path not found: {:?}\n\
            Please build Ceph first or set CEPH_PATH environment variable.\n\
            Expected path: {:?}",
            lib_path, lib_path
        );
    }

    // Add library search paths
    println!("cargo:rustc-link-search=native={}", lib_path.display());

    // Link against RGW libraries (static)
    // Order matters - dependencies must come after dependents
    println!("cargo:rustc-link-lib=static=rgw_common");
    println!("cargo:rustc-link-lib=static=rgw_a");

    // Link against Ceph shared libraries
    println!("cargo:rustc-link-lib=dylib=ceph-common");
    println!("cargo:rustc-link-lib=dylib=rados");

    // Link against system libraries required by Ceph
    // C++ standard library (Ceph is written in C++)
    println!("cargo:rustc-link-lib=dylib=stdc++");

    // OpenSSL is required for cryptographic operations
    println!("cargo:rustc-link-lib=dylib=crypto");
    println!("cargo:rustc-link-lib=dylib=ssl");

    // Additional system libraries commonly needed by Ceph
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=resolv");

    // Set rpath so libraries can be found at runtime
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());

    // Rerun if CEPH_PATH changes
    println!("cargo:rerun-if-env-changed=CEPH_PATH");

    // Print configuration for debugging
    println!("cargo:warning=RGW support enabled, linking against: {}", lib_path.display());
}
