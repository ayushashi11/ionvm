use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::OnceLock;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple};
use inkwell::OptimizationLevel;

type CUdevice = i32;
type CUdeviceptr = u64;
type CUcontext = *mut c_void;
type CUmodule = *mut c_void;
type CUfunction = *mut c_void;
type CUstream = *mut c_void;
type CUresult = u32;

const CUDA_SUCCESS: CUresult = 0;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: i32 = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: i32 = 76;

#[derive(Debug, Clone)]
pub struct CudaDeviceInfo {
    pub name: String,
    pub major: i32,
    pub minor: i32,
    pub total_memory: usize,
    pub arch: String,
}

#[allow(dead_code)]
struct CudaDriverLib {
    _handle: *mut c_void,
    cu_init: unsafe extern "C" fn(flags: u32) -> CUresult,
    cu_driver_get_version: unsafe extern "C" fn(version: *mut i32) -> CUresult,
    cu_device_get_count: unsafe extern "C" fn(count: *mut i32) -> CUresult,
    cu_device_get: unsafe extern "C" fn(device: *mut CUdevice, ordinal: i32) -> CUresult,
    cu_device_get_name: unsafe extern "C" fn(name: *mut c_char, len: i32, dev: CUdevice) -> CUresult,
    cu_device_get_attribute: unsafe extern "C" fn(pi: *mut i32, attrib: i32, dev: CUdevice) -> CUresult,
    cu_device_total_mem: unsafe extern "C" fn(bytes: *mut usize, dev: CUdevice) -> CUresult,
    cu_ctx_create: unsafe extern "C" fn(pctx: *mut CUcontext, flags: u32, dev: CUdevice) -> CUresult,
    cu_ctx_destroy: unsafe extern "C" fn(ctx: CUcontext) -> CUresult,
    cu_module_load_data: unsafe extern "C" fn(module: *mut CUmodule, image: *const c_void) -> CUresult,
    cu_module_unload: unsafe extern "C" fn(module: CUmodule) -> CUresult,
    cu_module_get_function: unsafe extern "C" fn(hfunc: *mut CUfunction, hmod: CUmodule, name: *const c_char) -> CUresult,
    cu_mem_alloc: unsafe extern "C" fn(dptr: *mut CUdeviceptr, bytesize: usize) -> CUresult,
    cu_mem_free: unsafe extern "C" fn(dptr: CUdeviceptr) -> CUresult,
    cu_memcpy_h_to_d: unsafe extern "C" fn(dst: CUdeviceptr, src: *const c_void, bytes: usize) -> CUresult,
    cu_memcpy_d_to_h: unsafe extern "C" fn(dst: *mut c_void, src: CUdeviceptr, bytes: usize) -> CUresult,
    cu_launch_kernel: unsafe extern "C" fn(
        f: CUfunction,
        grid_dim_x: u32,
        grid_dim_y: u32,
        grid_dim_z: u32,
        block_dim_x: u32,
        block_dim_y: u32,
        block_dim_z: u32,
        shared_mem_bytes: u32,
        h_stream: CUstream,
        kernel_params: *mut *mut c_void,
        extra: *mut *mut c_void,
    ) -> CUresult,
    cu_ctx_synchronize: unsafe extern "C" fn() -> CUresult,
}

unsafe impl Send for CudaDriverLib {}
unsafe impl Sync for CudaDriverLib {}

static DRIVER: OnceLock<Option<CudaDriverLib>> = OnceLock::new();

fn get_driver() -> Option<&'static CudaDriverLib> {
    DRIVER.get_or_init(|| {
        let paths = ["libcuda.so.1", "libcuda.so"];
        let mut handle = std::ptr::null_mut();
        for path in &paths {
            let c_path = CString::new(*path).unwrap();
            handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL) };
            if !handle.is_null() {
                break;
            }
        }
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_sym {
                ($sym:expr, $ty:ty) => {{
                    let s = CString::new($sym).unwrap();
                    let ptr = libc::dlsym(handle, s.as_ptr());
                    if ptr.is_null() {
                        libc::dlclose(handle);
                        return None;
                    }
                    let typed_fn: $ty = std::mem::transmute(ptr);
                    typed_fn
                }};
            }

            let cu_init = load_sym!("cuInit", unsafe extern "C" fn(flags: u32) -> CUresult);
            let cu_driver_get_version = load_sym!("cuDriverGetVersion", unsafe extern "C" fn(version: *mut i32) -> CUresult);
            let cu_device_get_count = load_sym!("cuDeviceGetCount", unsafe extern "C" fn(count: *mut i32) -> CUresult);
            let cu_device_get = load_sym!("cuDeviceGet", unsafe extern "C" fn(device: *mut CUdevice, ordinal: i32) -> CUresult);
            let cu_device_get_name = load_sym!("cuDeviceGetName", unsafe extern "C" fn(name: *mut c_char, len: i32, dev: CUdevice) -> CUresult);
            let cu_device_get_attribute = load_sym!("cuDeviceGetAttribute", unsafe extern "C" fn(pi: *mut i32, attrib: i32, dev: CUdevice) -> CUresult);
            let cu_device_total_mem = load_sym!("cuDeviceTotalMem_v2", unsafe extern "C" fn(bytes: *mut usize, dev: CUdevice) -> CUresult);
            let cu_ctx_create = load_sym!("cuCtxCreate_v2", unsafe extern "C" fn(pctx: *mut CUcontext, flags: u32, dev: CUdevice) -> CUresult);
            let cu_ctx_destroy = load_sym!("cuCtxDestroy_v2", unsafe extern "C" fn(ctx: CUcontext) -> CUresult);
            let cu_module_load_data = load_sym!("cuModuleLoadData", unsafe extern "C" fn(module: *mut CUmodule, image: *const c_void) -> CUresult);
            let cu_module_unload = load_sym!("cuModuleUnload", unsafe extern "C" fn(module: CUmodule) -> CUresult);
            let cu_module_get_function = load_sym!("cuModuleGetFunction", unsafe extern "C" fn(hfunc: *mut CUfunction, hmod: CUmodule, name: *const c_char) -> CUresult);
            let cu_mem_alloc = load_sym!("cuMemAlloc_v2", unsafe extern "C" fn(dptr: *mut CUdeviceptr, bytesize: usize) -> CUresult);
            let cu_mem_free = load_sym!("cuMemFree_v2", unsafe extern "C" fn(dptr: CUdeviceptr) -> CUresult);
            let cu_memcpy_h_to_d = load_sym!("cuMemcpyHtoD_v2", unsafe extern "C" fn(dst: CUdeviceptr, src: *const c_void, bytes: usize) -> CUresult);
            let cu_memcpy_d_to_h = load_sym!("cuMemcpyDtoH_v2", unsafe extern "C" fn(dst: *mut c_void, src: CUdeviceptr, bytes: usize) -> CUresult);
            let cu_launch_kernel = load_sym!("cuLaunchKernel", unsafe extern "C" fn(
                f: CUfunction,
                grid_dim_x: u32,
                grid_dim_y: u32,
                grid_dim_z: u32,
                block_dim_x: u32,
                block_dim_y: u32,
                block_dim_z: u32,
                shared_mem_bytes: u32,
                h_stream: CUstream,
                kernel_params: *mut *mut c_void,
                extra: *mut *mut c_void,
            ) -> CUresult);
            let cu_ctx_synchronize = load_sym!("cuCtxSynchronize", unsafe extern "C" fn() -> CUresult);

            // Initialize CUDA driver
            if (cu_init)(0) != CUDA_SUCCESS {
                libc::dlclose(handle);
                return None;
            }

            Some(CudaDriverLib {
                _handle: handle,
                cu_init,
                cu_driver_get_version,
                cu_device_get_count,
                cu_device_get,
                cu_device_get_name,
                cu_device_get_attribute,
                cu_device_total_mem,
                cu_ctx_create,
                cu_ctx_destroy,
                cu_module_load_data,
                cu_module_unload,
                cu_module_get_function,
                cu_mem_alloc,
                cu_mem_free,
                cu_memcpy_h_to_d,
                cu_memcpy_d_to_h,
                cu_launch_kernel,
                cu_ctx_synchronize,
            })
        }
    }).as_ref()
}

pub fn is_cuda_available() -> bool {
    get_driver().is_some()
}

pub fn get_cuda_device_info(ordinal: i32) -> Result<CudaDeviceInfo, String> {
    let drv = get_driver().ok_or_else(|| "CUDA driver (libcuda.so.1) not found or initialization failed".to_string())?;

    unsafe {
        let mut count = 0;
        let res = (drv.cu_device_get_count)(&mut count);
        if res != CUDA_SUCCESS || count == 0 {
            return Err("No CUDA-capable devices found".to_string());
        }

        if ordinal >= count {
            return Err(format!("Device ordinal {} out of range (total: {})", ordinal, count));
        }

        let mut dev: CUdevice = 0;
        let res = (drv.cu_device_get)(&mut dev, ordinal);
        if res != CUDA_SUCCESS {
            return Err(format!("cuDeviceGet failed with error code {}", res));
        }

        let mut name_buf = [0i8; 256];
        (drv.cu_device_get_name)(name_buf.as_mut_ptr(), 256, dev);
        let name = CStr::from_ptr(name_buf.as_ptr()).to_string_lossy().to_string();

        let mut major = 0;
        let mut minor = 0;
        (drv.cu_device_get_attribute)(&mut major, CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR, dev);
        (drv.cu_device_get_attribute)(&mut minor, CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR, dev);

        let mut total_mem = 0usize;
        (drv.cu_device_total_mem)(&mut total_mem, dev);

        let arch = format!("sm_{}{}", major, minor);

        Ok(CudaDeviceInfo {
            name,
            major,
            minor,
            total_memory: total_mem,
            arch,
        })
    }
}

pub fn emit_ptx_from_module(module: &inkwell::module::Module, arch: &str) -> Result<String, String> {
    Target::initialize_nvptx(&InitializationConfig::default());
    let triple = TargetTriple::create("nvptx64-nvidia-cuda");
    let target = Target::from_triple(&triple).map_err(|e| format!("Failed to create NVPTX target: {:?}", e))?;

    let target_machine = target
        .create_target_machine(
            &triple,
            arch,
            "+ptx80",
            OptimizationLevel::Aggressive,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("Failed to create NVPTX TargetMachine for {}", arch))?;

    let buffer = target_machine
        .write_to_memory_buffer(module, FileType::Assembly)
        .map_err(|e| format!("Failed to emit PTX assembly: {:?}", e))?;

    let ptx_code = std::str::from_utf8(buffer.as_slice())
        .map_err(|e| format!("Invalid UTF-8 in emitted PTX: {}", e))?;

    Ok(ptx_code.to_string())
}

pub struct CudaContext {
    ctx: CUcontext,
}

impl CudaContext {
    pub fn new(ordinal: i32) -> Result<Self, String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        unsafe {
            let mut dev: CUdevice = 0;
            let res = (drv.cu_device_get)(&mut dev, ordinal);
            if res != CUDA_SUCCESS {
                return Err(format!("cuDeviceGet failed: {}", res));
            }
            let mut ctx: CUcontext = std::ptr::null_mut();
            let res = (drv.cu_ctx_create)(&mut ctx, 0, dev);
            if res != CUDA_SUCCESS {
                return Err(format!("cuCtxCreate failed: {}", res));
            }
            Ok(CudaContext { ctx })
        }
    }

    pub fn load_ptx(&self, ptx: &str) -> Result<CudaModule, String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        let c_ptx = CString::new(ptx).map_err(|e| format!("Invalid PTX string: {}", e))?;
        unsafe {
            let mut module: CUmodule = std::ptr::null_mut();
            let res = (drv.cu_module_load_data)(&mut module, c_ptx.as_ptr() as *const c_void);
            if res != CUDA_SUCCESS {
                return Err(format!("cuModuleLoadData failed with error {}", res));
            }
            Ok(CudaModule { module })
        }
    }

    pub fn allocate_device_memory(&self, size_bytes: usize) -> Result<CUdeviceptr, String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        unsafe {
            let mut dptr: CUdeviceptr = 0;
            let res = (drv.cu_mem_alloc)(&mut dptr, size_bytes);
            if res != CUDA_SUCCESS {
                return Err(format!("cuMemAlloc failed: {}", res));
            }
            Ok(dptr)
        }
    }

    pub fn free_device_memory(&self, dptr: CUdeviceptr) {
        if let Some(drv) = get_driver() {
            unsafe {
                let _ = (drv.cu_mem_free)(dptr);
            }
        }
    }

    pub fn copy_to_device(&self, dst: CUdeviceptr, src: &[u8]) -> Result<(), String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        unsafe {
            let res = (drv.cu_memcpy_h_to_d)(dst, src.as_ptr() as *const c_void, src.len());
            if res != CUDA_SUCCESS {
                return Err(format!("cuMemcpyHtoD failed: {}", res));
            }
            Ok(())
        }
    }

    pub fn copy_to_host(&self, dst: &mut [u8], src: CUdeviceptr) -> Result<(), String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        unsafe {
            let res = (drv.cu_memcpy_d_to_h)(dst.as_mut_ptr() as *mut c_void, src, dst.len());
            if res != CUDA_SUCCESS {
                return Err(format!("cuMemcpyDtoH failed: {}", res));
            }
            Ok(())
        }
    }

    pub fn synchronize(&self) -> Result<(), String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        unsafe {
            let res = (drv.cu_ctx_synchronize)();
            if res != CUDA_SUCCESS {
                return Err(format!("cuCtxSynchronize failed: {}", res));
            }
            Ok(())
        }
    }
}

impl Drop for CudaContext {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            if let Some(drv) = get_driver() {
                unsafe {
                    let _ = (drv.cu_ctx_destroy)(self.ctx);
                }
            }
        }
    }
}

pub struct CudaModule {
    module: CUmodule,
}

impl CudaModule {
    pub fn get_function(&self, name: &str) -> Result<CUfunction, String> {
        let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
        let c_name = CString::new(name).map_err(|e| format!("Invalid kernel name: {}", e))?;
        unsafe {
            let mut func: CUfunction = std::ptr::null_mut();
            let res = (drv.cu_module_get_function)(&mut func, self.module, c_name.as_ptr());
            if res != CUDA_SUCCESS {
                return Err(format!("cuModuleGetFunction '{}' failed: {}", name, res));
            }
            Ok(func)
        }
    }
}

impl Drop for CudaModule {
    fn drop(&mut self) {
        if !self.module.is_null() {
            if let Some(drv) = get_driver() {
                unsafe {
                    let _ = (drv.cu_module_unload)(self.module);
                }
            }
        }
    }
}

pub unsafe fn launch_kernel(
    func: CUfunction,
    grid_dim: (u32, u32, u32),
    block_dim: (u32, u32, u32),
    shared_mem: u32,
    params: &mut [*mut c_void],
) -> Result<(), String> {
    let drv = get_driver().ok_or_else(|| "CUDA driver not available".to_string())?;
    unsafe {
        let res = (drv.cu_launch_kernel)(
            func,
            grid_dim.0,
            grid_dim.1,
            grid_dim.2,
            block_dim.0,
            block_dim.1,
            block_dim.2,
            shared_mem,
            std::ptr::null_mut(),
            params.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        if res != CUDA_SUCCESS {
            return Err(format!("cuLaunchKernel failed: {}", res));
        }
        Ok(())
    }
}
