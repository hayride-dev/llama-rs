# llama-rs

LLama-rs uses [rust-bindgen](https://rust-lang.github.io/rust-bindgen/) to generate bindings for llama.cpp. 

Currently, only the system bindings are generated. Higher level bindings can use the system bindings to implement a friendly, safe wrapper around LLama.cpp. 

# Build Targets 

LLama-rs takes advantage of LLama.cpp's cmake build files and focuses laregly on passing the correct flags to llama.cpp. 

### MacOS 

Mac os is the only existing build target for `llama-rs`. Other targets can be added by including them through the `build.rs` within the system bindings crate. 

