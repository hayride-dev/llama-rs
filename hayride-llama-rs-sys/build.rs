use std::{env,fs,io};
use std::path::{PathBuf, Path};
use std::process::Command;

use cmake::Config;
use glob::glob;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("BUILD_DEBUG").is_ok() {
            // cargo::warning=MESSAGE — Displays a warning on the terminal.
            println!("cargo:warning=[DEBUG] {}", format!($($arg)*));
        }
    };
}

fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_all(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

fn get_cargo_target_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let profile = std::env::var("PROFILE")?;
    let mut target_dir = None;
    let mut sub_path = out_dir.as_path();
    while let Some(parent) = sub_path.parent() {
        if parent.ends_with(&profile) {
            target_dir = Some(parent);
            break;
        }
        sub_path = parent;
    }
    let target_dir = target_dir.ok_or("not found")?;
    Ok(target_dir.to_path_buf())
}

fn macos_link_search_path() -> Option<String> {
    let output = Command::new("clang")
        .arg("--print-search-dirs")
        .output()
        .ok()?;
    if !output.status.success() {
        println!(
            "failed to run 'clang --print-search-dirs', continuing without a link search path"
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("libraries: =") {
            let path = line.split('=').nth(1)?;
            return Some(format!("{}/lib/darwin", path));
        }
    }

    println!("failed to determine link search path, continuing without it");
    None
}

fn main() {
    // get build envs 
    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap(); // release/debug
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_dir = manifest_dir.join("target").join(&profile);

    let llama_cpp_src = manifest_dir.join("llama.cpp");
    let llama_cpp_dst = out_dir.join("llama.cpp");

    let build_shared_libs = cfg!(feature = "cuda") || cfg!(feature = "dynamic-link");
    let build_shared_libs = std::env::var("LLAMA_BUILD_SHARED_LIBS")
        .map(|v| v == "1")
        .unwrap_or(build_shared_libs);
 
    let llama_lib_profile = env::var("LLAMA_LIB_PROFILE").unwrap_or("Release".to_string());
    let llama_static_crt = env::var("LLAMA_STATIC_CRT").map(|v| v == "1").unwrap_or(false);

    debug_log!("Arc Target: {}", target);
    debug_log!("Output Directory: {}", out_dir.display());
    debug_log!("Build Shared: {}", build_shared_libs);
    debug_log!("Profile: {}", profile);
    debug_log!("Target Directory: {}", target_dir.display());
    debug_log!("Manifest Directory: {}", manifest_dir.display());
    debug_log!("Llama Cpp Src: {}", llama_cpp_src.display());
    debug_log!("Llama Cpp Dst: {}", llama_cpp_dst.display());
    debug_log!("Llama Lib Profile: {}", llama_lib_profile);
    debug_log!("Llama Static CRT: {}", llama_static_crt);

    // Copy llama_cpp_src directory to llama_cpp_dst
    if !llama_cpp_dst.exists() {
        copy_dir_all(&llama_cpp_src, &llama_cpp_dst).expect("Failed to copy llama.cpp source directory");
    }

    // increase build speed of cmake
    env::set_var("CMAKE_BUILD_PARALLEL_LEVEL",
        std::thread::available_parallelism()
            .unwrap()
            .get()
            .to_string(),
    );

    // bindings
    let bindings_path = out_dir.join("bindings.rs");
    let bindings = bindgen::Builder::default()
            .header("wrapper.h")
            .clang_arg(format!("-I{}", llama_cpp_dst.join("include").display()))
            .clang_arg(format!("-I{}", llama_cpp_dst.join("ggml/include").display()))
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .derive_partialeq(true)
            .allowlist_function("ggml_.*")
            .allowlist_type("ggml_.*")
            .allowlist_function("llama_.*")
            .allowlist_type("llama_.*")
            .prepend_enum_name(false)
            .generate()
            .expect("Failed to generate bindings");
    bindings.write_to_file(bindings_path).expect("Couldn't write bindings!");
    
    debug_log!("Bindings Generated");

    // Build llama.cpp with cmake
    let mut config = Config::new(&llama_cpp_dst);

    // skip extra compilation 
    // TODO :: Create a Makefile that builds/kicks tests off 
    // for llama.cpp directly 
    config.define("LLAMA_BUILD_TESTS", "OFF");
    config.define("LLAMA_BUILD_EXAMPLES", "OFF");
    config.define("LLAMA_BUILD_TOOLS", "OFF");
    config.define("LLAMA_BUILD_SERVER", "OFF");
    config.define(
        "BUILD_SHARED_LIBS",
        if build_shared_libs { "ON" } else { "OFF" },
    );

    if cfg!(feature = "cuda") {
        config.define("GGML_CUDA", "ON");
    }

    // set cmake config options
    config.profile(&profile)
        .very_verbose(true)
        .always_configure(false);

    let build_dir = config.build();

    // Search paths
    println!("cargo:rustc-link-search={}", out_dir.join("lib").display());
    println!("cargo:rustc-link-search={}", build_dir.display());

    // Link libraries 
    let llama_libs_kind = if build_shared_libs { "dylib" } else { "static" };
    let lib_pattern = if cfg!(windows) {
        "*.lib"
    } else if cfg!(target_os = "macos") {
        if build_shared_libs {
            "*.dylib"
        } else {
            "*.a"
        }
    } else if build_shared_libs {
        "*.so"
    } else {
        "*.a"
    };

    let libs_dir = out_dir.join("lib");
    let pattern = libs_dir.join(lib_pattern);

    debug_log!("Linking libraries from: {}", pattern.display());
    for entry in glob(pattern.to_str().unwrap()).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                let stem = path.file_stem().unwrap();
                let stem_str = stem.to_str().unwrap();

                // Remove the "lib" prefix if present
                let lib_name = stem_str.strip_prefix("lib").unwrap_or(stem_str);

                debug_log!("LINK {}",format!("cargo:rustc-link-lib={}={}", llama_libs_kind, lib_name));
                println!("{}",format!("cargo:rustc-link-lib={}={}", llama_libs_kind, lib_name));
            }
            Err(e) => println!("cargo:warning=error={}", e),
        }
    }
    // macOS
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=MetalKit");
        println!("cargo:rustc-link-lib=framework=Accelerate");
        println!("cargo:rustc-link-lib=c++");

        // For older OSX link against clang
        if target.ends_with("-apple-darwin") {
            if let Some(path) = macos_link_search_path() {
                println!("cargo:rustc-link-lib=clang_rt.osx");
                println!("cargo:rustc-link-search={}", path);
            }
        }
    }

    // Linux
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    if target.contains("gnu") {
        println!("cargo:rustc-link-lib=gomp");
    }

    let dest_dir = get_cargo_target_dir().unwrap();
    println!("cargo:rustc-link-search={}", dest_dir.display());

    // copy DLLs to target
    if build_shared_libs {
        let shared_lib_pattern = if cfg!(windows) {
            "*.dll"
        } else if cfg!(target_os = "macos") {
            "*.dylib"
        } else {
            "*.so"
        };

        let shared_libs_dir = if cfg!(windows) { "bin" } else { "lib" };
        let libs_dir = out_dir.join(shared_libs_dir);
        let pattern = libs_dir.join(shared_lib_pattern);
        debug_log!("Extract lib assets {}", pattern.display());
        let mut libs_assets = Vec::new();

        for entry in glob(pattern.to_str().unwrap()).unwrap() {
            match entry {
                Ok(path) => {
                    libs_assets.push(path);
                }
                Err(e) => eprintln!("cargo:warning=error={}", e),
            }
        }

        for asset in libs_assets {
            let asset_clone = asset.clone();
            let filename = asset_clone.file_name().unwrap();
            let filename = filename.to_str().unwrap();
            let dst = dest_dir.join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }

            // Copy DLLs to examples as well
            if dest_dir.join("examples").exists() {
                let dst = target_dir.join("examples").join(filename);
                debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
                if !dst.exists() {
                    std::fs::hard_link(asset.clone(), dst).unwrap();
                }
            }

            // Copy DLLs to target/profile/deps as well for tests
            let dst = dest_dir.join("deps").join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }
        }
    }
}