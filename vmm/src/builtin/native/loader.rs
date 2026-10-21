use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vm_ffi::{FfiRegistry, RawFfiFunction};

use super::c_abi::{IonCFunction, IonCFunctionPtr, IonCPluginInitFn};
use super::rust_lib::RustPluginRegisterFn;

pub const IONVM_PLUGIN_MAGIC: u32 = 0x494F4E56; // "IONV"

static LOADED_LIBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static REGISTERED_C_FUNCS: Mutex<Vec<IonCFunction>> = Mutex::new(Vec::new());
static REGISTERED_RUST_PLUGINS: Mutex<Vec<RustPluginRegisterFn>> = Mutex::new(Vec::new());
static NATIVE_VERSION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

pub fn get_native_version() -> usize {
    NATIVE_VERSION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Resolve library name to an absolute or canonical path
fn resolve_library_path(lib_name: &str) -> Option<PathBuf> {
    let p = Path::new(lib_name);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }

    // Check current directory
    if p.exists() {
        if let Ok(canon) = std::fs::canonicalize(p) {
            return Some(canon);
        }
    }

    // Check LD_LIBRARY_PATH
    if let Ok(ld_path) = std::env::var("LD_LIBRARY_PATH") {
        for dir in ld_path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = Path::new(dir).join(lib_name);
            if candidate.exists() {
                if let Ok(canon) = std::fs::canonicalize(candidate) {
                    return Some(canon);
                }
            }
            if !lib_name.ends_with(".so") {
                let candidate_so = Path::new(dir).join(format!("{}.so", lib_name));
                if candidate_so.exists() {
                    if let Ok(canon) = std::fs::canonicalize(candidate_so) {
                        return Some(canon);
                    }
                }
            }
        }
    }

    // Check ./target/debug and ./target/release
    for target_dir in &["target/debug", "target/release"] {
        let candidate = Path::new(target_dir).join(lib_name);
        if candidate.exists() {
            if let Ok(canon) = std::fs::canonicalize(candidate) {
                return Some(canon);
            }
        }
    }

    None
}

/// Callback passed to C plugins during registration
unsafe extern "C" fn c_register_callback(
    _reg_ctx: *mut c_void,
    name: *const c_char,
    func: IonCFunctionPtr,
    arity: usize,
    ret_type: u8,
) -> i32 {
    if name.is_null() {
        return -1;
    }
    let fn_name = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };

    let c_func = IonCFunction::new(fn_name, func, arity, ret_type);

    let mut list = REGISTERED_C_FUNCS.lock().unwrap();
    list.push(c_func);
    NATIVE_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    0
}

/// Load a native library declared in bytecode metadata
pub fn load_native_library(lib_name: &str, _namespace: &str) -> Result<(), String> {
    let resolved_path = resolve_library_path(lib_name).unwrap_or_else(|| PathBuf::from(lib_name));

    let path_str = resolved_path.to_string_lossy().to_string();

    {
        let mut loaded = LOADED_LIBS.lock().unwrap();
        let set = loaded.get_or_insert_with(HashSet::new);
        if set.contains(&path_str) {
            return Ok(()); // Already loaded
        }
        set.insert(path_str.clone());
    }

    let c_path = CString::new(path_str.as_bytes()).map_err(|e| e.to_string())?;

    unsafe {
        let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            let err_ptr = libc::dlerror();
            let err_msg = if !err_ptr.is_null() {
                CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
            } else {
                "Unknown dlopen error".to_string()
            };
            return Err(format!("Failed to dlopen '{}': {}", path_str, err_msg));
        }

        // Verify magic number
        let magic_sym = libc::dlsym(handle, b"IONVM_PLUGIN_MAGIC\0".as_ptr() as *const c_char);
        if magic_sym.is_null() {
            return Err(format!(
                "Library '{}' does not export IONVM_PLUGIN_MAGIC",
                path_str
            ));
        }
        let magic = *(magic_sym as *const u32);
        if magic != IONVM_PLUGIN_MAGIC {
            return Err(format!(
                "Invalid plugin magic in '{}': expected 0x{:08X}, got 0x{:08X}",
                path_str, IONVM_PLUGIN_MAGIC, magic
            ));
        }

        // Check if library is a Rust library (rlib / Rust plugin)
        let is_rust_plugin = !libc::dlsym(handle, b"IONVM_RUST_PLUGIN\0".as_ptr() as *const c_char).is_null()
            || !libc::dlsym(handle, b"ionvm_rust_register\0".as_ptr() as *const c_char).is_null();

        if is_rust_plugin {
            // Load as Rust library using native Rust ABI (`extern "Rust"`)
            let sym = libc::dlsym(handle, b"ionvm_rust_register\0".as_ptr() as *const c_char);
            let sym = if sym.is_null() {
                libc::dlsym(handle, b"run_register\0".as_ptr() as *const c_char)
            } else {
                sym
            };

            if sym.is_null() {
                return Err(format!(
                    "Rust library '{}' does not export ionvm_rust_register or run_register",
                    path_str
                ));
            }

            let register_fn: RustPluginRegisterFn = std::mem::transmute(sym);
            REGISTERED_RUST_PLUGINS.lock().unwrap().push(register_fn);
            NATIVE_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            // Register into current thread registry
            crate::builtin::ffi::with_ffi_registry(|reg| {
                register_fn(reg);
            });
        } else {
            // Load as C ABI library (C, C++, Fortran, Pascal)
            let sym = libc::dlsym(handle, b"ionvm_plugin_init\0".as_ptr() as *const c_char);
            let sym = if sym.is_null() {
                libc::dlsym(handle, b"run_register\0".as_ptr() as *const c_char)
            } else {
                sym
            };

            if sym.is_null() {
                return Err(format!(
                    "C library '{}' does not export ionvm_plugin_init or run_register",
                    path_str
                ));
            }

            // Inject host yield function pointer if the C plugin supports it
            let set_yield_sym = libc::dlsym(handle, b"ion_set_yield_fn\0".as_ptr() as *const c_char);
            if !set_yield_sym.is_null() {
                let set_yield_fn: unsafe extern "C" fn(super::fiber::IonCYieldFn) =
                    std::mem::transmute(set_yield_sym);
                set_yield_fn(super::fiber::ion_c_yield);
            }

            let init_fn: IonCPluginInitFn = std::mem::transmute(sym);
            let status = init_fn(std::ptr::null_mut(), c_register_callback);
            if status != 0 {
                return Err(format!(
                    "Plugin initialization in '{}' failed with code {}",
                    path_str, status
                ));
            }

            // Register into current thread registry
            crate::builtin::ffi::with_ffi_registry(|reg| {
                sync_registered_natives(reg);
            });
        }
    }

    Ok(())
}

/// Synchronize all loaded native functions (both C and Rust) into the given registry
pub fn sync_registered_natives(reg: &mut FfiRegistry) {
    // 1. Sync C ABI functions
    let c_funcs = REGISTERED_C_FUNCS.lock().unwrap().clone();
    for func in c_funcs {
        if reg.get_function_info(func.name()).is_none() {
            reg.register(func);
        }
    }

    // 2. Sync Rust library functions
    let rust_plugins = REGISTERED_RUST_PLUGINS.lock().unwrap().clone();
    for rust_fn in rust_plugins {
        unsafe {
            rust_fn(reg);
        }
    }
}

/// Register an internal Rust plugin function pointer across all VM threads
pub fn register_rust_plugin(register_fn: RustPluginRegisterFn) {
    REGISTERED_RUST_PLUGINS.lock().unwrap().push(register_fn);
    NATIVE_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    crate::builtin::ffi::with_ffi_registry(|reg| {
        unsafe {
            register_fn(reg);
        }
    });
}

/// Get a list of registered C functions for inspection
pub fn get_registered_c_functions() -> Vec<IonCFunction> {
    REGISTERED_C_FUNCS.lock().unwrap().clone()
}
