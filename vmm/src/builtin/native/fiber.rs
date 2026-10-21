use std::alloc::{alloc, dealloc, Layout};
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::slice;

use super::c_abi::{IonCFunctionPtr, IonCValue};
use crate::lang::types::Value;
use vm_ffi::{FfiError, FfiResult, FfiReturn, FfiValue};

const FIBER_STACK_SIZE: usize = 128 * 1024;

core::arch::global_asm!(
    ".global ion_context_switch",
    ".type ion_context_switch, @function",
    "ion_context_switch:",
    "    push rbx",
    "    push rbp",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov [rdi], rsp",
    "    mov rsp, [rsi]",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbp",
    "    pop rbx",
    "    ret",
);

unsafe extern "C" {
    fn ion_context_switch(from_rsp: *mut *mut u8, to_rsp: *const *mut u8);
}

pub struct YieldRequest {
    pub callback_name: String,
    pub c_args: Vec<IonCValue>,
    pub out_ptr: *mut IonCValue,
}

thread_local! {
    static CURRENT_FIBER: Cell<*mut Fiber> = const { Cell::new(std::ptr::null_mut()) };
}

/// Trampoline executed when a fiber starts running
unsafe extern "C" fn fiber_entry_trampoline() {
    let fiber_ptr = CURRENT_FIBER.get();
    let fiber = unsafe { &mut *fiber_ptr };

    let mut out = IonCValue::nil();
    let status = unsafe { (fiber.func)(fiber.c_args.as_ptr(), fiber.c_args.len(), &mut out) };

    fiber.final_out = out;
    fiber.final_status = status;
    fiber.finished = true;

    // Switch back to host
    unsafe {
        ion_context_switch(&mut fiber.fiber_rsp, &fiber.main_rsp);
    }
}

pub type IonCYieldFn =
    unsafe extern "C" fn(*const c_char, *const IonCValue, usize, *mut IonCValue) -> i32;

/// Public C-ABI function callable from foreign C/C++/Fortran/Pascal code to yield to IonVM
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ion_c_yield(
    callback_name: *const c_char,
    args: *const IonCValue,
    argc: usize,
    out: *mut IonCValue,
) -> i32 {
    if callback_name.is_null() {
        return -1;
    }

    let cb_name = match unsafe { CStr::from_ptr(callback_name) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };

    let c_args = if args.is_null() || argc == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(args, argc) }.to_vec()
    };

    let fiber_ptr = CURRENT_FIBER.get();
    if fiber_ptr.is_null() {
        return -3;
    }
    let fiber = unsafe { &mut *fiber_ptr };

    fiber.yield_request = Some(YieldRequest {
        callback_name: cb_name,
        c_args,
        out_ptr: out,
    });

    // Switch back to host VM
    unsafe {
        ion_context_switch(&mut fiber.fiber_rsp, &fiber.main_rsp);
    }

    0 // Wakes up here after VM executes callback!
}

/// A Fiber instance maintained directly by an IonCFunction struct.
pub struct Fiber {
    func: IonCFunctionPtr,
    fn_name: String,
    stack_base: *mut u8,
    main_rsp: *mut u8,
    fiber_rsp: *mut u8,
    c_args: Vec<IonCValue>,
    yield_request: Option<YieldRequest>,
    final_out: IonCValue,
    final_status: i32,
    finished: bool,
}

impl Fiber {
    pub fn new(func: IonCFunctionPtr, fn_name: String) -> Result<Self, String> {
        let layout = Layout::from_size_align(FIBER_STACK_SIZE, 16).map_err(|e| e.to_string())?;
        let stack_base = unsafe { alloc(layout) };
        if stack_base.is_null() {
            return Err("Failed to allocate fiber stack".to_string());
        }

        let stack_top = unsafe { stack_base.add(FIBER_STACK_SIZE) };
        let initial_rsp = unsafe {
            let trampoline_ptr = stack_top.sub(16) as *mut usize;
            *trampoline_ptr = fiber_entry_trampoline as *const () as usize;

            let regs_ptr = stack_top.sub(64) as *mut usize;
            std::ptr::write_bytes(regs_ptr, 0, 6);
            stack_top.sub(64)
        };

        Ok(Self {
            func,
            fn_name,
            stack_base,
            main_rsp: std::ptr::null_mut(),
            fiber_rsp: initial_rsp,
            c_args: Vec::new(),
            yield_request: None,
            final_out: IonCValue::nil(),
            final_status: 0,
            finished: false,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn execute(&mut self, args: &[IonCValue]) -> FfiResult {
        self.c_args = args.to_vec();

        // Loop executing this fiber and servicing its yield requests
        loop {
            // Set CURRENT_FIBER for thread-local access during ion_c_yield
            CURRENT_FIBER.set(self as *mut Fiber);

            // Switch to fiber
            unsafe {
                ion_context_switch(&mut self.main_rsp, &self.fiber_rsp);
            }

            // Clear CURRENT_FIBER
            CURRENT_FIBER.set(std::ptr::null_mut());

            if let Some(req) = self.yield_request.take() {
                // Convert C args to IonVM Value args
                let mut vm_args = Vec::with_capacity(req.c_args.len());
                for c_val in &req.c_args {
                    let ffi_val = c_val.to_ffi_value().unwrap_or(FfiValue::Unit);
                    let val = match ffi_val {
                        FfiValue::Integer(i) => Value::Int(i),
                        FfiValue::Float(f) => Value::Float(f),
                        FfiValue::Number(n) => Value::Float(n),
                        FfiValue::Boolean(b) => Value::Bool(b),
                        FfiValue::String(s) | FfiValue::Atom(s) => Value::String(s),
                        _ => Value::Nil,
                    };
                    vm_args.push(val);
                }

                // Call IonVM native / builtin function
                let result_val = crate::builtin::call_native(&req.callback_name, &vm_args)
                    .unwrap_or(Value::Nil);

                // Write back result into C out pointer
                if !req.out_ptr.is_null() {
                    unsafe {
                        match result_val {
                            Value::Int(i) => *req.out_ptr = IonCValue::int(i),
                            Value::Float(f) => *req.out_ptr = IonCValue::float(f),
                            Value::Bool(b) => *req.out_ptr = IonCValue::bool_val(b),
                            Value::String(ref s) => {
                                let cs = CString::new(s.as_str()).unwrap_or_default();
                                *req.out_ptr = IonCValue::string(cs.into_raw());
                            }
                            _ => *req.out_ptr = IonCValue::nil(),
                        }
                    }
                }

                // Resume fiber with the result
                continue;
            }

            if self.finished {
                if self.final_status != 0 {
                    return Err(FfiError::RuntimeError(format!(
                        "C function '{}' failed with status {}",
                        self.fn_name, self.final_status
                    )));
                }

                return self.final_out
                    .to_ffi_value()
                    .map(FfiReturn::Value)
                    .map_err(|e| FfiError::RuntimeError(format!("Failed to marshal C return: {}", e)));
            }
        }
    }
}

impl Drop for Fiber {
    fn drop(&mut self) {
        if !self.stack_base.is_null() {
            let layout = Layout::from_size_align(FIBER_STACK_SIZE, 16).unwrap();
            unsafe {
                dealloc(self.stack_base, layout);
            }
        }
    }
}

unsafe impl Send for Fiber {}
