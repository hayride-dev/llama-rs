#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_llama() {
        unsafe {
            llama_backend_init();
            let val = llama_max_devices();
            // Log value
            println!("Max devices: {}", val);
        }
    }
}
