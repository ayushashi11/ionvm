use vm_ffi::FfiRegistry;

/// Direct Rust library registration function pointer using native Rust ABI (`extern "Rust"`).
///
/// When a plugin is written in Rust, it does not need any C marshaling overhead.
/// It directly receives `&mut FfiRegistry` and registers any struct implementing
/// `vm_ffi::FfiFunction` natively.
pub type RustPluginRegisterFn = unsafe extern "Rust" fn(registry: &mut FfiRegistry);
