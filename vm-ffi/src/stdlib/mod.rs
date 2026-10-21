//! Modular standard library for the VM FFI
//!
//! This module organizes standard library functions into logical groups
//! for better maintainability and optional inclusion.
//! Note: String operations are handled natively in IonVM bytecode via SIMD opcodes
//! (SUpperRR, SLowerRR, SCountByteRRR, SEqRRR, SFindRRR).

pub mod chafa;
pub mod fs;
pub mod hash;
pub mod io;
pub mod json;
pub mod math;
pub mod net;
pub mod os;
pub mod string;
pub mod time;

use crate::FfiRegistry;

/// Register all standard library functions
pub fn register_all(registry: &mut FfiRegistry) {
    math::register_math_functions(registry);
    io::register_io_functions(registry);
    time::register_time_functions(registry);
    os::register_os_functions(registry);
    fs::register_fs_functions(registry);
    hash::register_hash_functions(registry);
    chafa::register_chafa_functions(registry);
    string::register_string_functions(registry);
    json::register_json_functions(registry);
    net::register_net_functions(registry);
}

/// Register only math functions
pub fn register_math_only(registry: &mut FfiRegistry) {
    math::register_math_functions(registry);
}

/// Register only I/O functions
pub fn register_io_only(registry: &mut FfiRegistry) {
    io::register_io_functions(registry);
}

/// Register only time functions
pub fn register_time_only(registry: &mut FfiRegistry) {
    time::register_time_functions(registry);
}

/// Register only OS functions
pub fn register_os_only(registry: &mut FfiRegistry) {
    os::register_os_functions(registry);
}

/// Register only file system functions
pub fn register_fs_only(registry: &mut FfiRegistry) {
    fs::register_fs_functions(registry);
}

/// Register only hash functions
pub fn register_hash_only(registry: &mut FfiRegistry) {
    hash::register_hash_functions(registry);
}

/// Get a list of all available modules
pub fn available_modules() -> Vec<&'static str> {
    vec!["math", "io", "time", "os", "fs", "hash", "chafa", "string", "json", "net"]
}

/// Register functions from specific modules
pub fn register_modules(registry: &mut FfiRegistry, modules: &[&str]) {
    for module in modules {
        match *module {
            "math" => math::register_math_functions(registry),
            "io" => io::register_io_functions(registry),
            "time" => time::register_time_functions(registry),
            "os" => os::register_os_functions(registry),
            "fs" => fs::register_fs_functions(registry),
            "hash" => hash::register_hash_functions(registry),
            "chafa" => chafa::register_chafa_functions(registry),
            "string" => string::register_string_functions(registry),
            "json" => json::register_json_functions(registry),
            "net" => net::register_net_functions(registry),
            _ => eprintln!("Warning: Unknown module '{}'", module),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_all() {
        let mut registry = FfiRegistry::new();
        register_all(&mut registry);

        // Check that functions from all modules are registered
        assert!(registry.get_function_info("Sqrt").is_some()); // math
        assert!(registry.get_function_info("Print").is_some()); // io
        assert!(registry.get_function_info("OsName").is_some()); // os
        assert!(registry.get_function_info("Crc32").is_some()); // hash

        let functions = registry.list_functions();
        assert!(functions.len() > 10); // Should have many functions
    }

    #[test]
    fn test_register_selective() {
        let mut registry = FfiRegistry::new();
        register_math_only(&mut registry);

        // Should have math functions but not I/O or os
        assert!(registry.get_function_info("Sqrt").is_some());
        assert!(registry.get_function_info("Print").is_none());
        assert!(registry.get_function_info("OsName").is_none());
    }

    #[test]
    fn test_register_modules() {
        let mut registry = FfiRegistry::new();
        register_modules(&mut registry, &["math"]);

        assert!(registry.get_function_info("Sqrt").is_some());
        assert!(registry.get_function_info("Print").is_none());
        assert!(registry.get_function_info("OsName").is_none());
    }

    #[test]
    fn test_available_modules() {
        let modules = available_modules();
        assert!(modules.contains(&"math"));
        assert!(modules.contains(&"io"));
        assert!(modules.contains(&"os"));
        assert!(modules.contains(&"fs"));
        assert!(modules.contains(&"hash"));
        assert!(modules.contains(&"string"));
        assert!(modules.contains(&"json"));
        assert!(modules.contains(&"net"));
    }
}
