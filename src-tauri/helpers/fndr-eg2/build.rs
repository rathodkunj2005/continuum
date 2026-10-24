//! Links the llama.cpp that scripts/eg2/build-llama.sh built (pinned tag,
//! patched) and generates bindings for the parts the embedder calls.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let llama = env::var_os("FNDR_LLAMA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("vendor/llama"));
    let include = llama.join("include");
    let lib = llama.join("lib");
    println!("cargo:rerun-if-env-changed=FNDR_LLAMA_DIR");
    println!("cargo:rerun-if-changed={}", llama.join("BUILD").display());
    println!("cargo:rerun-if-changed=wrapper.h");
    if !lib.join("libllamacpp.a").is_file() {
        panic!(
            "no llama.cpp build at {}: run scripts/eg2/build-llama.sh first",
            llama.display()
        );
    }
    let stamp = std::fs::read_to_string(llama.join("BUILD")).unwrap_or_default();
    println!(
        "cargo:rustc-env=FNDR_LLAMA_BUILD={}",
        stamp.split_whitespace().next().unwrap_or("unknown")
    );

    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=llamacpp");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=c++");
        for framework in ["Accelerate", "Foundation", "Metal", "MetalKit"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=m");
    }

    let bindings = bindgen::Builder::default()
        .header(manifest.join("wrapper.h").to_string_lossy())
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function("llama_.*")
        .allowlist_type("llama_.*")
        .allowlist_var("LLAMA_.*")
        .prepend_enum_name(false)
        .layout_tests(false)
        .generate()
        .expect("bindgen over llama.h");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");
    bindings.write_to_file(out).expect("write bindings");
}
