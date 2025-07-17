//! Modular standard library for the VM FFI
//!
//! This module organizes standard library functions into logical groups
//! for better maintainability and optional inclusion.

pub mod io;
pub mod math;
pub mod string;

use crate::FfiRegistry;

/// Register all standard library functions
pub fn register_all(registry: &mut FfiRegistry) {
    math::register_math_functions(registry);
    io::register_io_functions(registry);
    string::register_string_functions(registry);
}

/// Register only math functions
pub fn register_math_only(registry: &mut FfiRegistry) {
    math::register_math_functions(registry);
}

/// Register only I/O functions
pub fn register_io_only(registry: &mut FfiRegistry) {
    io::register_io_functions(registry);
}

/// Register only string functions
pub fn register_string_only(registry: &mut FfiRegistry) {
    string::register_string_functions(registry);
}

/// Get a list of all available modules
pub fn available_modules() -> Vec<&'static str> {
    vec!["math", "io", "string"]
}

/// Register functions from specific modules
pub fn register_modules(registry: &mut FfiRegistry, modules: &[&str]) {
    for module in modules {
        match *module {
            "math" => math::register_math_functions(registry),
            "io" => io::register_io_functions(registry),
            "string" => string::register_string_functions(registry),
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
        assert!(registry.get_function_info("StrLength").is_some()); // string

        let functions = registry.list_functions();
        assert!(functions.len() > 10); // Should have many functions
    }

    #[test]
    fn test_register_selective() {
        let mut registry = FfiRegistry::new();
        register_math_only(&mut registry);

        // Should have math functions but not I/O or string
        assert!(registry.get_function_info("Sqrt").is_some());
        assert!(registry.get_function_info("Print").is_none());
        assert!(registry.get_function_info("StrLength").is_none());
    }

    #[test]
    fn test_register_modules() {
        let mut registry = FfiRegistry::new();
        register_modules(&mut registry, &["math"]);

        assert!(registry.get_function_info("Sqrt").is_some());
        assert!(registry.get_function_info("Print").is_none());
        assert!(registry.get_function_info("StrLength").is_none());
    }

    #[test]
    fn test_available_modules() {
        let modules = available_modules();
        assert!(modules.contains(&"math"));
        assert!(modules.contains(&"io"));
        assert!(modules.contains(&"string"));
    }
}
