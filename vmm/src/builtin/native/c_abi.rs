use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;
use vm_ffi::{FfiContext, FfiError, FfiFunction, FfiResult, FfiType, FfiValue, RawFfiFunction};

use super::fiber::Fiber;

pub const ION_VAL_NIL: i32 = 0;
pub const ION_VAL_INT: i32 = 1;
pub const ION_VAL_FLOAT: i32 = 2;
pub const ION_VAL_BOOL: i32 = 3;
pub const ION_VAL_STRING: i32 = 4;
pub const ION_VAL_POINTER: i32 = 5;

/// C-ABI compatible tagged value for interoperability with C, C++, Fortran, Pascal, etc.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct IonCValue {
    pub tag: i32,
    pub _reserved: i32,
    pub data: IonCData,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union IonCData {
    pub int_val: i64,
    pub float_val: f64,
    pub bool_val: i32,
    pub string_val: *const c_char,
    pub ptr_val: *mut c_void,
}

impl IonCValue {
    pub fn nil() -> Self {
        Self {
            tag: ION_VAL_NIL,
            _reserved: 0,
            data: IonCData { int_val: 0 },
        }
    }

    pub fn int(v: i64) -> Self {
        Self {
            tag: ION_VAL_INT,
            _reserved: 0,
            data: IonCData { int_val: v },
        }
    }

    pub fn float(v: f64) -> Self {
        Self {
            tag: ION_VAL_FLOAT,
            _reserved: 0,
            data: IonCData { float_val: v },
        }
    }

    pub fn bool_val(v: bool) -> Self {
        Self {
            tag: ION_VAL_BOOL,
            _reserved: 0,
            data: IonCData {
                bool_val: if v { 1 } else { 0 },
            },
        }
    }

    pub fn string(ptr: *const c_char) -> Self {
        Self {
            tag: ION_VAL_STRING,
            _reserved: 0,
            data: IonCData { string_val: ptr },
        }
    }

    pub fn pointer(ptr: *mut c_void) -> Self {
        Self {
            tag: ION_VAL_POINTER,
            _reserved: 0,
            data: IonCData { ptr_val: ptr },
        }
    }

    /// Convert from IonCValue to FfiValue
    pub fn to_ffi_value(&self) -> Result<FfiValue, String> {
        match self.tag {
            ION_VAL_NIL => Ok(FfiValue::Unit),
            ION_VAL_INT => Ok(FfiValue::Integer(unsafe { self.data.int_val })),
            ION_VAL_FLOAT => Ok(FfiValue::Float(unsafe { self.data.float_val })),
            ION_VAL_BOOL => Ok(FfiValue::Boolean(unsafe { self.data.bool_val != 0 })),
            ION_VAL_STRING => {
                let ptr = unsafe { self.data.string_val };
                if ptr.is_null() {
                    Ok(FfiValue::String(String::new()))
                } else {
                    let cstr = unsafe { CStr::from_ptr(ptr) };
                    Ok(FfiValue::String(cstr.to_string_lossy().into_owned()))
                }
            }
            _ => Ok(FfiValue::Unit),
        }
    }
}

/// C function pointer signature for foreign functions.
/// `args`: Pointer to array of arguments
/// `argc`: Number of arguments
/// `out`: Output value buffer provided by caller
/// Returns: 0 on success, non-zero error code on failure
pub type IonCFunctionPtr =
    unsafe extern "C" fn(args: *const IonCValue, argc: usize, out: *mut IonCValue) -> i32;

/// Registration callback passed to the C library during plugin initialization
pub type IonCRegisterCallback = unsafe extern "C" fn(
    reg_ctx: *mut c_void,
    name: *const c_char,
    func: IonCFunctionPtr,
    arity: usize,
    ret_type: u8,
) -> i32;

/// Plugin entry point exported by C-ABI plugins
pub type IonCPluginInitFn =
    unsafe extern "C" fn(reg_ctx: *mut c_void, reg_cb: IonCRegisterCallback) -> i32;

/// IonCFunction: Implements `vm_ffi::FfiFunction` for C-ABI functions.
/// All translations between IonVM / Rust types and standard C ABI happen here.
pub struct IonCFunction {
    pub name: String,
    pub func: IonCFunctionPtr,
    pub arity: usize,
    pub ret_type: FfiType,
    pub fiber: Mutex<Option<Fiber>>,
}

impl Clone for IonCFunction {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            func: self.func,
            arity: self.arity,
            ret_type: self.ret_type,
            fiber: Mutex::new(None),
        }
    }
}

impl IonCFunction {
    pub fn new(name: String, func: IonCFunctionPtr, arity: usize, ret_type_u8: u8) -> Self {
        let ret_type = match ret_type_u8 {
            0 => FfiType::Any,
            1 => FfiType::Void,
            2 => FfiType::Int,
            3 => FfiType::Float,
            4 => FfiType::Boolean,
            5 => FfiType::String,
            6 => FfiType::Tuple,
            7 => FfiType::Array,
            8 => FfiType::Object,
            _ => FfiType::Any,
        };
        Self {
            name,
            func,
            arity,
            ret_type,
            fiber: Mutex::new(None),
        }
    }
}

impl RawFfiFunction for IonCFunction {
    fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
        // Keep CStrings alive for the duration of the C call
        let mut string_storage: Vec<CString> = Vec::new();
        let mut c_args: Vec<IonCValue> = Vec::with_capacity(args.len());

        for arg in &args {
            match arg {
                FfiValue::Integer(i) => c_args.push(IonCValue::int(*i)),
                FfiValue::Float(f) => c_args.push(IonCValue::float(*f)),
                FfiValue::Number(n) => c_args.push(IonCValue::float(*n)),
                FfiValue::Boolean(b) => c_args.push(IonCValue::bool_val(*b)),
                FfiValue::String(s) | FfiValue::Atom(s) => {
                    let cs = CString::new(s.as_bytes()).unwrap_or_default();
                    let ptr = cs.as_ptr();
                    string_storage.push(cs);
                    c_args.push(IonCValue::string(ptr));
                }
                FfiValue::Unit | FfiValue::Undefined => c_args.push(IonCValue::nil()),
                _ => c_args.push(IonCValue::nil()),
            }
        }

        let mut fiber_cell = self.fiber.lock().unwrap();
        if fiber_cell.is_none() || fiber_cell.as_ref().unwrap().is_finished() {
            *fiber_cell = Some(Fiber::new(self.func, self.name.clone())
                .map_err(|e| FfiError::RuntimeError(e))?);
        }
        fiber_cell.as_mut().unwrap().execute(&c_args)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn arity(&self) -> usize {
        self.arity
    }

    fn return_type(&self) -> FfiType {
        self.ret_type
    }
}

impl FfiFunction for IonCFunction {}
