use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::passes::PassBuilderOptions;
use inkwell::values::{FloatValue, IntValue, PointerValue, VectorValue};
use inkwell::AddressSpace;
use inkwell::{FloatPredicate, IntPredicate};
use inkwell::OptimizationLevel;
use std::sync::Mutex;

use crate::bytecode::instruction::OperandType;
use crate::bytecode::opcode::Opcode;
use crate::lang::matcher::{ListSegment, Pattern};
use crate::lang::types::{Function, NativeJitFn, Value};

struct RawJitRoot(*mut Context, *mut ExecutionEngine<'static>);
unsafe impl Send for RawJitRoot {}
unsafe impl Sync for RawJitRoot {}

static JIT_ROOTS: Mutex<Vec<RawJitRoot>> = Mutex::new(Vec::new());

pub fn cleanup_jit_engines() {
    if let Ok(mut roots) = JIT_ROOTS.lock() {
        for RawJitRoot(ctx_ptr, ee_ptr) in roots.drain(..) {
            unsafe {
                drop(Box::from_raw(ee_ptr));
                drop(Box::from_raw(ctx_ptr));
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_get_obj_prop_int(obj_val_ptr: *const Value, idx: i64) -> i64 {
    if obj_val_ptr.is_null() || idx < 0 {
        return 0;
    }
    let u_idx = idx as usize;
    match unsafe { &*obj_val_ptr } {
        Value::IntArray(arr) => {
            if let Ok(borrowed) = arr.read() {
                if u_idx < borrowed.len() {
                    return borrowed[u_idx];
                }
            }
        }
        Value::Array(arr) => {
            if let Ok(borrowed) = arr.read() {
                if let Some(Value::Int(v)) = borrowed.get(u_idx) {
                    return *v;
                }
            }
        }
        Value::Object(obj) => {
            if let Ok(borrowed) = obj.read() {
                if let Some(Some(prop)) = borrowed.indexed.get(u_idx) {
                    if let Value::Int(v) = prop.value {
                        return v;
                    }
                }
                if u_idx < 10 {
                    let key = [b'0' + u_idx as u8];
                    if let Ok(key_str) = std::str::from_utf8(&key) {
                        if let Some(prop) = borrowed.properties.get(key_str) {
                            if let Value::Int(v) = prop.value {
                                return v;
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_copy_value(dst: *mut Value, src: *const Value) {
    if dst.is_null() || src.is_null() {
        return;
    }
    unsafe {
        *dst = (*src).clone();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_get_enum_tag(val_ptr: *const Value) -> i64 {
    if val_ptr.is_null() {
        return -1;
    }
    match unsafe { &*val_ptr } {
        Value::Object(obj) => {
            if let Ok(borrowed) = obj.read() {
                if let Some(Value::Int(tag)) = borrowed.get_property("@tag") {
                    return tag;
                }
            }
        }
        _ => {}
    }
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_get_intarray_len(val_ptr: *const Value) -> i64 {
    if val_ptr.is_null() {
        return 0;
    }
    match unsafe { &*val_ptr } {
        Value::IntArray(arr) => {
            if let Ok(borrowed) = arr.read() {
                return borrowed.len() as i64;
            }
        }
        _ => {}
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_get_intarray_data_ptr(val_ptr: *const Value) -> *const i64 {
    if val_ptr.is_null() {
        return std::ptr::null();
    }
    if let Value::IntArray(arr) = unsafe { &*val_ptr } {
        if let Ok(borrowed) = arr.read() {
            return borrowed.as_ptr();
        }
    }
    std::ptr::null()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_get_floatarray_data_ptr(val_ptr: *const Value) -> *const f64 {
    if val_ptr.is_null() {
        return std::ptr::null();
    }
    if let Value::FloatArray(arr) = unsafe { &*val_ptr } {
        if let Ok(borrowed) = arr.read() {
            return borrowed.as_ptr();
        }
    }
    std::ptr::null()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_array_get_int(val_ptr: *const Value, idx: i64) -> i64 {
    if val_ptr.is_null() || idx < 0 {
        return 0;
    }
    match unsafe { &*val_ptr } {
        Value::IntArray(arr) => {
            if let Ok(borrowed) = arr.read() {
                let u = idx as usize;
                if u < borrowed.len() {
                    return borrowed[u];
                }
            }
        }
        Value::Array(arr) => {
            if let Ok(borrowed) = arr.read() {
                let u = idx as usize;
                if let Some(Value::Int(v)) = borrowed.get(u) {
                    return *v;
                }
            }
        }
        _ => {}
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_array_get_float(val_ptr: *const Value, idx: i64) -> f64 {
    if val_ptr.is_null() || idx < 0 {
        return 0.0;
    }
    match unsafe { &*val_ptr } {
        Value::FloatArray(arr) => {
            if let Ok(borrowed) = arr.read() {
                let u = idx as usize;
                if u < borrowed.len() {
                    return borrowed[u];
                }
            }
        }
        Value::Array(arr) => {
            if let Ok(borrowed) = arr.read() {
                let u = idx as usize;
                if let Some(Value::Float(v)) = borrowed.get(u) {
                    return *v;
                }
            }
        }
        _ => {}
    }
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_array_set_int(val_ptr: *const Value, idx: i64, val: i64) {
    if val_ptr.is_null() || idx < 0 {
        return;
    }
    match unsafe { &*val_ptr } {
        Value::IntArray(arr) => {
            if let Ok(mut borrowed) = arr.write() {
                let u = idx as usize;
                if u < borrowed.len() {
                    borrowed[u] = val;
                } else {
                    borrowed.resize(u + 1, 0);
                    borrowed[u] = val;
                }
            }
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_array_set_float(val_ptr: *const Value, idx: i64, val: f64) {
    if val_ptr.is_null() || idx < 0 {
        return;
    }
    match unsafe { &*val_ptr } {
        Value::FloatArray(arr) => {
            if let Ok(mut borrowed) = arr.write() {
                let u = idx as usize;
                if u < borrowed.len() {
                    borrowed[u] = val;
                } else {
                    borrowed.resize(u + 1, 0.0);
                    borrowed[u] = val;
                }
            }
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_eq(p1: *const Value, p2: *const Value) -> i64 {
    if p1.is_null() || p2.is_null() {
        return 0;
    }
    match (unsafe { &*p1 }, unsafe { &*p2 }) {
        (Value::String(a), Value::String(b)) => if crate::vm::simd::simd_str_eq(a, b) { 1 } else { 0 },
        (a, b) => if a.eql(b) { 1 } else { 0 },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_concat(p1: *const Value, p2: *const Value, dst: *mut Value) {
    if p1.is_null() || p2.is_null() || dst.is_null() {
        return;
    }
    let s1 = unsafe { &*p1 }.to_str();
    let s2 = unsafe { &*p2 }.to_str();
    let mut out = String::with_capacity(s1.len() + s2.len());
    out.push_str(&s1);
    out.push_str(&s2);
    unsafe {
        *dst = Value::String(out);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_find(haystack: *const Value, needle: *const Value) -> i64 {
    if haystack.is_null() || needle.is_null() {
        return -1;
    }
    match (unsafe { &*haystack }, unsafe { &*needle }) {
        (Value::String(h), Value::String(n)) => {
            crate::vm::simd::simd_str_find(h, n).map(|i| i as i64).unwrap_or(-1)
        }
        _ => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_count_byte(val: *const Value, byte: i64) -> i64 {
    if val.is_null() {
        return 0;
    }
    match unsafe { &*val } {
        Value::String(s) => crate::vm::simd::simd_str_count_byte(s, byte as u8) as i64,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_to_int(val: *const Value) -> i64 {
    if val.is_null() {
        return 0;
    }
    match unsafe { &*val } {
        Value::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_upper(val: *const Value, dst: *mut Value) {
    if val.is_null() || dst.is_null() {
        return;
    }
    match unsafe { &*val } {
        Value::String(s) => {
            let mut owned = s.clone();
            crate::vm::simd::simd_str_to_upper_mut(&mut owned);
            unsafe { *dst = Value::String(owned); }
        }
        other => unsafe { *dst = other.clone(); },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_lower(val: *const Value, dst: *mut Value) {
    if val.is_null() || dst.is_null() {
        return;
    }
    match unsafe { &*val } {
        Value::String(s) => {
            let mut owned = s.clone();
            crate::vm::simd::simd_str_to_lower_mut(&mut owned);
            unsafe { *dst = Value::String(owned); }
        }
        other => unsafe { *dst = other.clone(); },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vm_str_slice(val: *const Value, start: i64, end: i64, dst: *mut Value) {
    if val.is_null() || dst.is_null() {
        return;
    }
    match unsafe { &*val } {
        Value::String(s) => {
            let s_len = s.len();
            let start_idx = (start.max(0) as usize).min(s_len);
            let end_idx = if end < 0 { s_len } else { (end.max(0) as usize).min(s_len).max(start_idx) };
            unsafe { *dst = Value::String(s[start_idx..end_idx].to_string()); }
        }
        _ => unsafe { *dst = Value::String("".to_string()); },
    }
}

pub fn compile_with_llvm_ir(func: &Function) -> Option<(NativeJitFn, String)> {
    Target::initialize_native(&InitializationConfig::default()).ok()?;

    let context_ptr = Box::into_raw(Box::new(Context::create()));
    let context: &'static Context = unsafe { &*context_ptr };
    let module = context.create_module(&format!("jit_{}", func.name));
    let ee_ptr = match module.create_jit_execution_engine(OptimizationLevel::Aggressive) {
        Ok(engine) => Box::into_raw(Box::new(engine)),
        Err(_) => {
            unsafe { drop(Box::from_raw(context_ptr)); }
            return None;
        }
    };
    let ee: &'static ExecutionEngine<'static> = unsafe { &*ee_ptr };

    if let Ok(mut roots) = JIT_ROOTS.lock() {
        roots.push(RawJitRoot(context_ptr, ee_ptr));
    }

    let ptr_type = context.ptr_type(AddressSpace::default());
    let i64_type = context.i64_type();
    let f64_type = context.f64_type();
    let i8_type = context.i8_type();
    let _i1_type = context.bool_type();
    let void_type = context.void_type();

    let fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into(), ptr_type.into(), ptr_type.into(), ptr_type.into()], false);
    let fn_name = format!("jit_{}", func.name);
    let llvm_fn = module.add_function(&fn_name, fn_type, None);

    let copy_value_fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    let copy_value_fn = module.add_function("vm_copy_value", copy_value_fn_type, None);
    ee.add_global_mapping(&copy_value_fn, vm_copy_value as *const () as usize);

    let helper_fn_type = i64_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let helper_fn = module.add_function("vm_get_obj_prop_int", helper_fn_type, None);
    ee.add_global_mapping(&helper_fn, vm_get_obj_prop_int as *const () as usize);

    let get_int_ptr_fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
    let get_int_ptr_fn = module.add_function("vm_get_intarray_data_ptr", get_int_ptr_fn_type, None);
    ee.add_global_mapping(&get_int_ptr_fn, vm_get_intarray_data_ptr as *const () as usize);

    let get_flt_ptr_fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
    let get_flt_ptr_fn = module.add_function("vm_get_floatarray_data_ptr", get_flt_ptr_fn_type, None);
    ee.add_global_mapping(&get_flt_ptr_fn, vm_get_floatarray_data_ptr as *const () as usize);

    let arr_get_int_fn_type = i64_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let arr_get_int_fn = module.add_function("vm_array_get_int", arr_get_int_fn_type, None);
    ee.add_global_mapping(&arr_get_int_fn, vm_array_get_int as *const () as usize);

    let arr_get_flt_fn_type = f64_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let arr_get_flt_fn = module.add_function("vm_array_get_float", arr_get_flt_fn_type, None);
    ee.add_global_mapping(&arr_get_flt_fn, vm_array_get_float as *const () as usize);

    let arr_set_int_fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into(), i64_type.into()], false);
    let arr_set_int_fn = module.add_function("vm_array_set_int", arr_set_int_fn_type, None);
    ee.add_global_mapping(&arr_set_int_fn, vm_array_set_int as *const () as usize);

    let arr_set_flt_fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into(), f64_type.into()], false);
    let arr_set_flt_fn = module.add_function("vm_array_set_float", arr_set_flt_fn_type, None);
    ee.add_global_mapping(&arr_set_flt_fn, vm_array_set_float as *const () as usize);

    let get_enum_tag_fn_type = i64_type.fn_type(&[ptr_type.into()], false);
    let get_enum_tag_fn = module.add_function("vm_get_enum_tag", get_enum_tag_fn_type, None);
    ee.add_global_mapping(&get_enum_tag_fn, vm_get_enum_tag as *const () as usize);

    let get_intarray_len_fn_type = i64_type.fn_type(&[ptr_type.into()], false);
    let get_intarray_len_fn = module.add_function("vm_get_intarray_len", get_intarray_len_fn_type, None);
    ee.add_global_mapping(&get_intarray_len_fn, vm_get_intarray_len as *const () as usize);

    let str_eq_fn_type = i64_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    let str_eq_fn = module.add_function("vm_str_eq", str_eq_fn_type, None);
    ee.add_global_mapping(&str_eq_fn, vm_str_eq as *const () as usize);

    let str_concat_fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into(), ptr_type.into()], false);
    let str_concat_fn = module.add_function("vm_str_concat", str_concat_fn_type, None);
    ee.add_global_mapping(&str_concat_fn, vm_str_concat as *const () as usize);

    let str_find_fn_type = i64_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    let str_find_fn = module.add_function("vm_str_find", str_find_fn_type, None);
    ee.add_global_mapping(&str_find_fn, vm_str_find as *const () as usize);

    let str_count_byte_fn_type = i64_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let str_count_byte_fn = module.add_function("vm_str_count_byte", str_count_byte_fn_type, None);
    ee.add_global_mapping(&str_count_byte_fn, vm_str_count_byte as *const () as usize);

    let str_to_int_fn_type = i64_type.fn_type(&[ptr_type.into()], false);
    let str_to_int_fn = module.add_function("vm_str_to_int", str_to_int_fn_type, None);
    ee.add_global_mapping(&str_to_int_fn, vm_str_to_int as *const () as usize);

    let str_upper_fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    let str_upper_fn = module.add_function("vm_str_upper", str_upper_fn_type, None);
    ee.add_global_mapping(&str_upper_fn, vm_str_upper as *const () as usize);

    let str_lower_fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
    let str_lower_fn = module.add_function("vm_str_lower", str_lower_fn_type, None);
    ee.add_global_mapping(&str_lower_fn, vm_str_lower as *const () as usize);

    let str_slice_fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into(), i64_type.into(), ptr_type.into()], false);
    let str_slice_fn = module.add_function("vm_str_slice", str_slice_fn_type, None);
    ee.add_global_mapping(&str_slice_fn, vm_str_slice as *const () as usize);

    let builder = context.create_builder();
    let entry_bb = context.append_basic_block(llvm_fn, "entry");

    let inst_count = func.instructions.len();
    let mut bbs = Vec::with_capacity(inst_count);
    for i in 0..inst_count {
        bbs.push(context.append_basic_block(llvm_fn, &format!("bb_{}", i)));
    }

    builder.position_at_end(entry_bb);
    let v4i64_type = i64_type.vec_type(4);
    let v4f64_type = f64_type.vec_type(4);
    let i32_type = context.i32_type();

    let has_vector_ops = func.instructions.iter().any(|inst| matches!(
        inst.opcode,
        Opcode::VIotaI | Opcode::VSplatI | Opcode::VSplatF
        | Opcode::VAddI | Opcode::VSubI | Opcode::VMulI | Opcode::VSumI
        | Opcode::VAddIC | Opcode::VSubIC | Opcode::VMulIC
        | Opcode::VDivIC | Opcode::VRemIC | Opcode::VDivI | Opcode::VRemI | Opcode::VDivF
        | Opcode::VCmpEqI | Opcode::VCmpGtI | Opcode::VCmpLtI | Opcode::VExtractI
        | Opcode::VAddF | Opcode::VSubF | Opcode::VMulF | Opcode::VDotF | Opcode::VSumF
        | Opcode::VGatherI
    ));

    let mut reg_val_allocas = Vec::with_capacity(func.register_count);
    let mut reg_tag_allocas = Vec::with_capacity(func.register_count);
    let mut reg_v4i_allocas = Vec::with_capacity(if has_vector_ops { func.register_count } else { 0 });
    let mut reg_v4f_allocas = Vec::with_capacity(if has_vector_ops { func.register_count } else { 0 });
    for r in 0..func.register_count {
        let v_alloca = builder.build_alloca(i64_type, &format!("alloca_val_{}", r)).ok()?;
        let t_alloca = builder.build_alloca(i8_type, &format!("alloca_tag_{}", r)).ok()?;
        builder.build_store(v_alloca, i64_type.const_int(0, false)).ok()?;
        builder.build_store(t_alloca, i8_type.const_int(6, false)).ok()?;
        reg_val_allocas.push(v_alloca);
        reg_tag_allocas.push(t_alloca);

        if has_vector_ops {
            let v4i_alloca = builder.build_alloca(v4i64_type, &format!("alloca_v4i_{}", r)).ok()?;
            let v4f_alloca = builder.build_alloca(v4f64_type, &format!("alloca_v4f_{}", r)).ok()?;
            builder.build_store(v4i_alloca, v4i64_type.const_zero()).ok()?;
            builder.build_store(v4f_alloca, v4f64_type.const_zero()).ok()?;
            reg_v4i_allocas.push(v4i_alloca);
            reg_v4f_allocas.push(v4f_alloca);
        }
    }

    let regs_param = llvm_fn.get_nth_param(0)?.into_pointer_value();
    let self_param = llvm_fn.get_nth_param(1)?.into_pointer_value();
    let args_param = llvm_fn.get_nth_param(2)?.into_pointer_value();
    let env_param = llvm_fn.get_nth_param(3)?.into_pointer_value();
    let out_res_param = llvm_fn.get_nth_param(4)?.into_pointer_value();

    let val_size = std::mem::size_of::<Value>();

    let get_reg_ptr = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<PointerValue<'static>, inkwell::builder::BuilderError> {
        let offset = (reg_idx * val_size) as u64;
        let offset_val = i64_type.const_int(offset, false);
        unsafe { builder.build_gep(i8_type, regs_param, &[offset_val], "reg_ptr") }
    };

    let get_cap_ptr = |builder: &inkwell::builder::Builder<'static>, cap_idx: usize| -> Result<PointerValue<'static>, inkwell::builder::BuilderError> {
        let offset = (cap_idx * val_size) as u64;
        let offset_val = i64_type.const_int(offset, false);
        unsafe { builder.build_gep(i8_type, env_param, &[offset_val], "cap_ptr") }
    };

    let get_cap_int = |builder: &inkwell::builder::Builder<'static>, cap_idx: usize| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        let cap_ptr = get_cap_ptr(builder, cap_idx)?;
        let int_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, cap_ptr, &[int_offset], "val_ptr") }?;
        let loaded = builder.build_load(i64_type, val_ptr, "cap_int")?;
        Ok(loaded.into_int_value())
    };

    let set_cap_int = |builder: &inkwell::builder::Builder<'static>, cap_idx: usize, val: IntValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let cap_ptr = get_cap_ptr(builder, cap_idx)?;
        let tag = i8_type.const_int(0, false);
        builder.build_store(cap_ptr, tag)?;
        let int_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, cap_ptr, &[int_offset], "val_ptr") }?;
        builder.build_store(val_ptr, val)?;
        Ok(())
    };

    let get_cap_float = |builder: &inkwell::builder::Builder<'static>, cap_idx: usize| -> Result<FloatValue<'static>, inkwell::builder::BuilderError> {
        let cap_ptr = get_cap_ptr(builder, cap_idx)?;
        let flt_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, cap_ptr, &[flt_offset], "val_ptr") }?;
        let loaded = builder.build_load(f64_type, val_ptr, "cap_float")?;
        Ok(loaded.into_float_value())
    };

    let set_cap_float = |builder: &inkwell::builder::Builder<'static>, cap_idx: usize, val: FloatValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let cap_ptr = get_cap_ptr(builder, cap_idx)?;
        let tag = i8_type.const_int(1, false);
        builder.build_store(cap_ptr, tag)?;
        let flt_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, cap_ptr, &[flt_offset], "val_ptr") }?;
        builder.build_store(val_ptr, val)?;
        Ok(())
    };

    let get_reg_int = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        if reg_idx < reg_val_allocas.len() {
            let loaded = builder.build_load(i64_type, reg_val_allocas[reg_idx], &format!("r{}_int", reg_idx))?;
            Ok(loaded.into_int_value())
        } else {
            let reg_ptr = get_reg_ptr(builder, reg_idx)?;
            let int_offset = i64_type.const_int(8, false);
            let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[int_offset], "val_ptr") }?;
            let loaded = builder.build_load(i64_type, val_ptr, "reg_int")?;
            Ok(loaded.into_int_value())
        }
    };

    let set_reg_int = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: IntValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        if reg_idx < reg_val_allocas.len() {
            builder.build_store(reg_tag_allocas[reg_idx], i8_type.const_int(0, false))?;
            builder.build_store(reg_val_allocas[reg_idx], val)?;
            Ok(())
        } else {
            let reg_ptr = get_reg_ptr(builder, reg_idx)?;
            let tag = i8_type.const_int(0, false);
            builder.build_store(reg_ptr, tag)?;
            let int_offset = i64_type.const_int(8, false);
            let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[int_offset], "val_ptr") }?;
            builder.build_store(val_ptr, val)?;
            Ok(())
        }
    };

    let get_arg_ptr = |builder: &inkwell::builder::Builder<'static>, arg_idx: usize| -> Result<PointerValue<'static>, inkwell::builder::BuilderError> {
        let offset = (arg_idx * val_size) as u64;
        let offset_val = i64_type.const_int(offset, false);
        unsafe { builder.build_gep(i8_type, args_param, &[offset_val], "arg_ptr") }
    };

    let get_arg_int = |builder: &inkwell::builder::Builder<'static>, arg_idx: usize| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        let arg_ptr = get_arg_ptr(builder, arg_idx)?;
        let int_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, arg_ptr, &[int_offset], "val_ptr") }?;
        let loaded = builder.build_load(i64_type, val_ptr, "arg_int")?;
        Ok(loaded.into_int_value())
    };

    let get_reg_float = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<FloatValue<'static>, inkwell::builder::BuilderError> {
        if reg_idx < reg_val_allocas.len() {
            let loaded = builder.build_load(i64_type, reg_val_allocas[reg_idx], &format!("r{}_flt_raw", reg_idx))?;
            let flt = builder.build_bit_cast(loaded, f64_type, &format!("r{}_flt", reg_idx))?;
            Ok(flt.into_float_value())
        } else {
            let reg_ptr = get_reg_ptr(builder, reg_idx)?;
            let flt_offset = i64_type.const_int(8, false);
            let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[flt_offset], "val_ptr") }?;
            let loaded = builder.build_load(f64_type, val_ptr, "reg_float")?;
            Ok(loaded.into_float_value())
        }
    };

    let set_reg_float = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: FloatValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        if reg_idx < reg_val_allocas.len() {
            builder.build_store(reg_tag_allocas[reg_idx], i8_type.const_int(1, false))?;
            let raw_int = builder.build_bit_cast(val, i64_type, "flt_to_raw_int")?;
            builder.build_store(reg_val_allocas[reg_idx], raw_int)?;
            Ok(())
        } else {
            let reg_ptr = get_reg_ptr(builder, reg_idx)?;
            let tag = i8_type.const_int(1, false);
            builder.build_store(reg_ptr, tag)?;
            let flt_offset = i64_type.const_int(8, false);
            let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[flt_offset], "val_ptr") }?;
            builder.build_store(val_ptr, val)?;
            Ok(())
        }
    };

    let set_reg_bool = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: IntValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let zext = builder.build_int_z_extend(val, i64_type, "bool_zext")?;
        if reg_idx < reg_val_allocas.len() {
            builder.build_store(reg_tag_allocas[reg_idx], i8_type.const_int(2, false))?;
            builder.build_store(reg_val_allocas[reg_idx], zext)?;
            Ok(())
        } else {
            let reg_ptr = get_reg_ptr(builder, reg_idx)?;
            let tag = i8_type.const_int(2, false);
            builder.build_store(reg_ptr, tag)?;
            let bool_offset = i64_type.const_int(8, false);
            let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[bool_offset], "val_ptr") }?;
            builder.build_store(val_ptr, zext)?;
            Ok(())
        }
    };

    let get_reg_v4i = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<VectorValue<'static>, inkwell::builder::BuilderError> {
        let loaded = builder.build_load(v4i64_type, reg_v4i_allocas[reg_idx], &format!("r{}_v4i", reg_idx))?;
        Ok(loaded.into_vector_value())
    };

    let set_reg_v4i = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: VectorValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        builder.build_store(reg_tag_allocas[reg_idx], i8_type.const_int(10, false))?;
        builder.build_store(reg_v4i_allocas[reg_idx], val)?;
        Ok(())
    };

    let get_reg_v4f = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<VectorValue<'static>, inkwell::builder::BuilderError> {
        let loaded = builder.build_load(v4f64_type, reg_v4f_allocas[reg_idx], &format!("r{}_v4f", reg_idx))?;
        Ok(loaded.into_vector_value())
    };

    let set_reg_v4f = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: VectorValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        builder.build_store(reg_tag_allocas[reg_idx], i8_type.const_int(11, false))?;
        builder.build_store(reg_v4f_allocas[reg_idx], val)?;
        Ok(())
    };

    let resolve_int = |builder: &inkwell::builder::Builder<'static>, op: &OperandType| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        match op {
            OperandType::Register(r) => get_reg_int(builder, *r),
            OperandType::Argument(a) => get_arg_int(builder, *a),
            OperandType::Captured(c) => get_cap_int(builder, *c),
            OperandType::This => {
                let offset = i64_type.const_int(8, false);
                let val_ptr = unsafe { builder.build_gep(i8_type, self_param, &[offset], "this_val_ptr") }?;
                Ok(builder.build_load(i64_type, val_ptr, "this_int")?.into_int_value())
            },
            OperandType::Constant(Value::Int(c)) => Ok(i64_type.const_int(*c as u64, true)),
            OperandType::Constant(Value::Float(c)) => Ok(i64_type.const_int(*c as i64 as u64, true)),
            OperandType::Constant(Value::Bool(b)) => Ok(i64_type.const_int(if *b { 1 } else { 0 }, false)),
            _ => Ok(i64_type.const_int(0, false)),
        }
    };

    let get_arg_float = |builder: &inkwell::builder::Builder<'static>, arg_idx: usize| -> Result<FloatValue<'static>, inkwell::builder::BuilderError> {
        let arg_ptr = get_arg_ptr(builder, arg_idx)?;
        let flt_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, arg_ptr, &[flt_offset], "val_ptr") }?;
        let loaded = builder.build_load(f64_type, val_ptr, "arg_float")?;
        Ok(loaded.into_float_value())
    };

    let resolve_float = |builder: &inkwell::builder::Builder<'static>, op: &OperandType| -> Result<FloatValue<'static>, inkwell::builder::BuilderError> {
        match op {
            OperandType::Register(r) => get_reg_float(builder, *r),
            OperandType::Argument(a) => get_arg_float(builder, *a),
            OperandType::Captured(c) => get_cap_float(builder, *c),
            OperandType::This => {
                let offset = i64_type.const_int(8, false);
                let val_ptr = unsafe { builder.build_gep(i8_type, self_param, &[offset], "this_val_ptr") }?;
                Ok(builder.build_load(f64_type, val_ptr, "this_float")?.into_float_value())
            },
            OperandType::Constant(Value::Float(c)) => Ok(f64_type.const_float(*c)),
            OperandType::Constant(Value::Int(c)) => Ok(f64_type.const_float(*c as f64)),
            _ => Ok(f64_type.const_float(0.0)),
        }
    };

    let mut reg_to_arg = std::collections::HashMap::new();
    for inst in &func.instructions {
        if inst.opcode == Opcode::LoadConst && inst.operands.len() >= 2 {
            if let (OperandType::Argument(a), OperandType::Register(r)) = (&inst.operands[0], &inst.operands[1]) {
                reg_to_arg.insert(*r, *a);
            }
        }
    }

    let mut int_array_args = std::collections::HashSet::new();
    let mut float_array_args = std::collections::HashSet::new();
    for inst in &func.instructions {
        match inst.opcode {
            Opcode::IArrayGet
            | Opcode::IArraySet
            | Opcode::IBitGet
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0
            | Opcode::ArrayGet
            | Opcode::AGetIndexAR
            | Opcode::GetIndex
            | Opcode::VGatherI => {
                if let Some(op) = inst.operands.first() {
                    match op {
                        OperandType::Argument(a) => { int_array_args.insert(*a); }
                        OperandType::Register(r) => {
                            if let Some(&a) = reg_to_arg.get(r) {
                                int_array_args.insert(a);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Opcode::FArrayGet | Opcode::FArraySet => {
                if let Some(op) = inst.operands.first() {
                    match op {
                        OperandType::Argument(a) => { float_array_args.insert(*a); }
                        OperandType::Register(r) => {
                            if let Some(&a) = reg_to_arg.get(r) {
                                float_array_args.insert(a);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    builder.position_at_end(entry_bb);
    let mut arg_int_ptrs = std::collections::HashMap::new();
    for &a in &int_array_args {
        if a < func.argument_count {
            let arg_ptr = get_arg_ptr(&builder, a).ok()?;
            let call_int = builder.build_call(get_int_ptr_fn, &[arg_ptr.into()], &format!("arg_{}_int_ptr", a)).ok()?;
            let i_ptr = match call_int.try_as_basic_value() {
                inkwell::values::ValueKind::Basic(b) => b.into_pointer_value(),
                _ => return None,
            };
            arg_int_ptrs.insert(a, i_ptr);
        }
    }

    let mut arg_float_ptrs = std::collections::HashMap::new();
    for &a in &float_array_args {
        if a < func.argument_count {
            let arg_ptr = get_arg_ptr(&builder, a).ok()?;
            let call_flt = builder.build_call(get_flt_ptr_fn, &[arg_ptr.into()], &format!("arg_{}_float_ptr", a)).ok()?;
            let f_ptr = match call_flt.try_as_basic_value() {
                inkwell::values::ValueKind::Basic(b) => b.into_pointer_value(),
                _ => return None,
            };
            arg_float_ptrs.insert(a, f_ptr);
        }
    }

    if !bbs.is_empty() {
        let _ = builder.build_unconditional_branch(bbs[0]);
    } else {
        let _ = builder.build_return(None);
        return None;
    }

    for (i, inst) in func.instructions.iter().enumerate() {
        builder.position_at_end(bbs[i]);
        let ops = &inst.operands;

        match inst.opcode {
            Opcode::IAddRRR | Opcode::IAddRCR | Opcode::Add => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let sum = builder.build_int_add(l, r, "add_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, sum).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR | Opcode::Sub => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let diff = builder.build_int_sub(l, r, "sub_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, diff).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IMulRRR | Opcode::IMulRCR | Opcode::Mul => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let prod = builder.build_int_mul(l, r, "mul_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, prod).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IDivRCR | Opcode::IDivCRR | Opcode::Div => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let div = builder.build_int_signed_div(l, r, "div_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, div).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IRemRCR | Opcode::IRemCRR | Opcode::Rem => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let rem = builder.build_int_signed_rem(l, r, "rem_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, rem).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IAndRRR | Opcode::IAndRCR | Opcode::And => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let res = builder.build_and(l, r, "and_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IOrRRR | Opcode::IOrRCR | Opcode::Or => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let res = builder.build_or(l, r, "or_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IXorRRR | Opcode::IXorRCR | Opcode::Xor => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let res = builder.build_xor(l, r, "xor_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IShlRRR | Opcode::IShlRCR | Opcode::Shl => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let mask63 = i64_type.const_int(63, false);
                let shift_amt = builder.build_and(r, mask63, "shl_mask").ok()?;
                let res = builder.build_left_shift(l, shift_amt, "shl_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IShrRRR | Opcode::IShrRCR | Opcode::Shr => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let mask63 = i64_type.const_int(63, false);
                let shift_amt = builder.build_and(r, mask63, "shr_mask").ok()?;
                let res = builder.build_right_shift(l, shift_amt, true, "shr_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Not => {
                let val = resolve_int(&builder, &ops[0]).ok()?;
                let res = builder.build_not(val, "not_tmp").ok()?;
                if let OperandType::Register(dst) = ops[1] {
                    set_reg_int(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::LoadConst => {
                if let OperandType::Register(dst) = ops[1] {
                    match &ops[0] {
                        OperandType::Constant(Value::Float(f)) => {
                            let f_val = f64_type.const_float(*f);
                            set_reg_float(&builder, dst, f_val).ok()?;
                        }
                        OperandType::Constant(Value::Int(c)) => {
                            let i_val = i64_type.const_int(*c as u64, true);
                            set_reg_int(&builder, dst, i_val).ok()?;
                        }
                        OperandType::Constant(Value::Bool(b)) => {
                            let b_val = i64_type.const_int(if *b { 1 } else { 0 }, false);
                            set_reg_bool(&builder, dst, b_val).ok()?;
                        }
                        OperandType::Register(src) => {
                            if *src < reg_val_allocas.len() && dst < reg_val_allocas.len() {
                                let src_tag = builder.build_load(i8_type, reg_tag_allocas[*src], "src_tag").ok()?;
                                builder.build_store(reg_tag_allocas[dst], src_tag).ok()?;
                                let val = builder.build_load(i64_type, reg_val_allocas[*src], "val").ok()?;
                                builder.build_store(reg_val_allocas[dst], val).ok()?;
                            } else {
                                let src_ptr = get_reg_ptr(&builder, *src).ok()?;
                                let dst_ptr = get_reg_ptr(&builder, dst).ok()?;
                                let src_tag = builder.build_load(i8_type, src_ptr, "src_tag").ok()?;
                                builder.build_store(dst_ptr, src_tag).ok()?;
                                let val_offset = i64_type.const_int(8, false);
                                let src_val_ptr = unsafe { builder.build_gep(i8_type, src_ptr, &[val_offset], "src_val_ptr") }.ok()?;
                                let dst_val_ptr = unsafe { builder.build_gep(i8_type, dst_ptr, &[val_offset], "dst_val_ptr") }.ok()?;
                                let val = builder.build_load(i64_type, src_val_ptr, "val").ok()?;
                                builder.build_store(dst_val_ptr, val).ok()?;
                            }
                        }
                        OperandType::Argument(a) => {
                            let src_ptr = get_arg_ptr(&builder, *a).ok()?;
                            let src_tag = builder.build_load(i8_type, src_ptr, "src_tag").ok()?;
                            let val_offset = i64_type.const_int(8, false);
                            let src_val_ptr = unsafe { builder.build_gep(i8_type, src_ptr, &[val_offset], "src_val_ptr") }.ok()?;
                            let val = builder.build_load(i64_type, src_val_ptr, "val").ok()?;
                            if dst < reg_val_allocas.len() {
                                builder.build_store(reg_tag_allocas[dst], src_tag).ok()?;
                                builder.build_store(reg_val_allocas[dst], val).ok()?;
                            } else {
                                let dst_ptr = get_reg_ptr(&builder, dst).ok()?;
                                builder.build_store(dst_ptr, src_tag).ok()?;
                                let dst_val_ptr = unsafe { builder.build_gep(i8_type, dst_ptr, &[val_offset], "dst_val_ptr") }.ok()?;
                                builder.build_store(dst_val_ptr, val).ok()?;
                            }
                        }
                        OperandType::This => {
                            let dst_ptr = get_reg_ptr(&builder, dst).ok()?;
                            let _ = builder.build_call(
                                copy_value_fn,
                                &[dst_ptr.into(), self_param.into()],
                                "copy_this_to_reg",
                            ).ok()?;
                            if dst < reg_tag_allocas.len() {
                                let this_tag = builder.build_load(i8_type, self_param, "this_tag").ok()?;
                                builder.build_store(reg_tag_allocas[dst], this_tag).ok()?;
                            }
                        }
                        _ => {
                            let val = resolve_int(&builder, &ops[0]).ok()?;
                            set_reg_int(&builder, dst, val).ok()?;
                        }
                    }
                } else if let OperandType::Captured(cap_idx) = ops[1] {
                    match &ops[0] {
                        OperandType::Register(src) => {
                            let cap_ptr = get_cap_ptr(&builder, cap_idx).ok()?;
                            let val_offset = i64_type.const_int(8, false);
                            let cap_val_ptr = unsafe { builder.build_gep(i8_type, cap_ptr, &[val_offset], "cap_val_ptr") }.ok()?;
                            if *src < reg_val_allocas.len() {
                                let src_tag = builder.build_load(i8_type, reg_tag_allocas[*src], "src_tag").ok()?;
                                let val = builder.build_load(i64_type, reg_val_allocas[*src], "src_val").ok()?;
                                builder.build_store(cap_ptr, src_tag).ok()?;
                                builder.build_store(cap_val_ptr, val).ok()?;
                            } else {
                                let src_ptr = get_reg_ptr(&builder, *src).ok()?;
                                let src_tag = builder.build_load(i8_type, src_ptr, "src_tag").ok()?;
                                builder.build_store(cap_ptr, src_tag).ok()?;
                                let src_val_ptr = unsafe { builder.build_gep(i8_type, src_ptr, &[val_offset], "src_val_ptr") }.ok()?;
                                let val = builder.build_load(i64_type, src_val_ptr, "val").ok()?;
                                builder.build_store(cap_val_ptr, val).ok()?;
                            }
                        }
                        OperandType::This => {
                            let cap_ptr = get_cap_ptr(&builder, cap_idx).ok()?;
                            let _ = builder.build_call(
                                copy_value_fn,
                                &[cap_ptr.into(), self_param.into()],
                                "copy_this_to_capture",
                            ).ok()?;
                        }
                        OperandType::Constant(Value::Int(c)) => {
                            let i_val = i64_type.const_int(*c as u64, true);
                            set_cap_int(&builder, cap_idx, i_val).ok()?;
                        }
                        OperandType::Constant(Value::Float(f)) => {
                            let f_val = f64_type.const_float(*f);
                            set_cap_float(&builder, cap_idx, f_val).ok()?;
                        }
                        _ => {
                            let val = resolve_int(&builder, &ops[0]).ok()?;
                            set_cap_int(&builder, cap_idx, val).ok()?;
                        }
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Eq | Opcode::IEqRRR | Opcode::IEqRCR => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::EQ, l, r, "cmp_eq").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR | Opcode::ILtCRR => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::SLT, l, r, "cmp_lt").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::IGtCRR => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::SGT, l, r, "cmp_gt").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::JumpTrue | Opcode::IJumpTrueR => {
                let cond_val = resolve_int(&builder, &ops[0]).ok()?;
                let zero = i64_type.const_int(0, false);
                let is_true = builder.build_int_compare(IntPredicate::NE, cond_val, zero, "is_true").ok()?;
                let off = match ops[1] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(is_true, target_bb, next_bb);
            }
            Opcode::JumpFalse | Opcode::IJumpFalseR => {
                let cond_val = resolve_int(&builder, &ops[0]).ok()?;
                let zero = i64_type.const_int(0, false);
                let is_false = builder.build_int_compare(IntPredicate::EQ, cond_val, zero, "is_false").ok()?;
                let off = match ops[1] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(is_false, target_bb, next_bb);
            }
            Opcode::Jump => {
                let off = match ops[0] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let _ = builder.build_unconditional_branch(target_bb);
            }
            Opcode::IGtJmpRR => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::SGT, l, r, "gt_cmp").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::IGtJmpRC => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::SGT, l, r, "gt_cmp_c").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::ILtJmpRR | Opcode::ILtJmpRC => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::SLT, l, r, "lt_cmp").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::IEqJmpRR | Opcode::IEqJmpRC => {
                let l = resolve_int(&builder, &ops[0]).ok()?;
                let r = resolve_int(&builder, &ops[1]).ok()?;
                let cmp = builder.build_int_compare(IntPredicate::EQ, l, r, "eq_cmp").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::FAddRRR | Opcode::FAddRCR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let res = builder.build_float_add(l, r, "fadd").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_float(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FSubRRR | Opcode::FSubRCR | Opcode::FSubCRR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let res = builder.build_float_sub(l, r, "fsub").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_float(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FMulRRR | Opcode::FMulRCR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let res = builder.build_float_mul(l, r, "fmul").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_float(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FDivRRR | Opcode::FDivRCR | Opcode::FDivCRR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let res = builder.build_float_div(l, r, "fdiv").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_float(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FEqRRR | Opcode::FEqRCR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let cmp = builder.build_float_compare(FloatPredicate::OEQ, l, r, "f_eq_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FLtRRR | Opcode::FLtRCR | Opcode::FLtCRR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let cmp = builder.build_float_compare(FloatPredicate::OLT, l, r, "f_lt_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FGtRRR | Opcode::FGtRCR | Opcode::FGtCRR => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let cmp = builder.build_float_compare(FloatPredicate::OGT, l, r, "f_gt_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FGtJmpRR | Opcode::FGtJmpRC => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let cmp = builder.build_float_compare(FloatPredicate::OGT, l, r, "f_gt_cmp").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::FLtJmpRR | Opcode::FLtJmpRC => {
                let l = resolve_float(&builder, &ops[0]).ok()?;
                let r = resolve_float(&builder, &ops[1]).ok()?;
                let cmp = builder.build_float_compare(FloatPredicate::OLT, l, r, "f_lt_cmp").ok()?;
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::IArrayGet => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                let arg_idx = match ops[0] {
                    OperandType::Argument(a) => Some(a),
                    OperandType::Register(r) => reg_to_arg.get(&r).copied(),
                    _ => None,
                };
                if let Some(a) = arg_idx.filter(|a| arg_int_ptrs.contains_key(a)) {
                    let data_ptr = arg_int_ptrs[&a];
                    let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                    let direct_val = builder.build_load(i64_type, elem_ptr, "direct_val").ok()?.into_int_value();
                    if let OperandType::Register(dst) = ops[2] {
                        set_reg_int(&builder, dst, direct_val).ok()?;
                    }
                } else {
                    let arr_ptr = match ops[0] {
                        OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                        OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                        OperandType::This => self_param,
                        _ => get_arg_ptr(&builder, 0).ok()?,
                    };
                    let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), idx_val.into()], "arr_get_call").ok()?;
                    let res_val = match call_res.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    if let OperandType::Register(dst) = ops[2] {
                        set_reg_int(&builder, dst, res_val).ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IArraySet => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                let val_to_store = resolve_int(&builder, &ops[2]).ok()?;
                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                        let _ = builder.build_store(elem_ptr, val_to_store);
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let _ = builder.build_call(arr_set_int_fn, &[arr_ptr.into(), idx_val.into(), val_to_store.into()], "arr_set_call").ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IBitGet => {
                let bit_idx = resolve_int(&builder, &ops[1]).ok()?;
                let const_6 = i64_type.const_int(6, false);
                let const_63 = i64_type.const_int(63, false);
                let const_1 = i64_type.const_int(1, false);

                let word_idx = builder.build_right_shift(bit_idx, const_6, false, "bit_word_idx").ok()?;
                let bit_pos = builder.build_and(bit_idx, const_63, "bit_pos").ok()?;

                let word_val = match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[word_idx], "word_ptr") }.ok()?;
                        builder.build_load(i64_type, elem_ptr, "loaded_word").ok()?.into_int_value()
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), word_idx.into()], "arr_get_call").ok()?;
                        match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        }
                    }
                };

                let shifted = builder.build_right_shift(word_val, bit_pos, false, "bit_shifted").ok()?;
                let bit_res = builder.build_and(shifted, const_1, "bit_res").ok()?;

                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, bit_res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IBitSet1 => {
                let bit_idx = resolve_int(&builder, &ops[1]).ok()?;
                let const_6 = i64_type.const_int(6, false);
                let const_63 = i64_type.const_int(63, false);
                let const_1 = i64_type.const_int(1, false);

                let word_idx = builder.build_right_shift(bit_idx, const_6, false, "bit_word_idx").ok()?;
                let bit_pos = builder.build_and(bit_idx, const_63, "bit_pos").ok()?;
                let mask = builder.build_left_shift(const_1, bit_pos, "bit_mask").ok()?;

                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[word_idx], "word_ptr") }.ok()?;
                        let old_word = builder.build_load(i64_type, elem_ptr, "old_word").ok()?.into_int_value();
                        let new_word = builder.build_or(old_word, mask, "new_word").ok()?;
                        let _ = builder.build_store(elem_ptr, new_word);
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), word_idx.into()], "arr_get_call").ok()?;
                        let old_word = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        };
                        let new_word = builder.build_or(old_word, mask, "new_word").ok()?;
                        let _ = builder.build_call(arr_set_int_fn, &[arr_ptr.into(), word_idx.into(), new_word.into()], "arr_set_call").ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IBitSet0 => {
                let bit_idx = resolve_int(&builder, &ops[1]).ok()?;
                let const_6 = i64_type.const_int(6, false);
                let const_63 = i64_type.const_int(63, false);
                let const_1 = i64_type.const_int(1, false);

                let word_idx = builder.build_right_shift(bit_idx, const_6, false, "bit_word_idx").ok()?;
                let bit_pos = builder.build_and(bit_idx, const_63, "bit_pos").ok()?;
                let mask = builder.build_left_shift(const_1, bit_pos, "bit_mask").ok()?;
                let not_mask = builder.build_not(mask, "not_mask").ok()?;

                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[word_idx], "word_ptr") }.ok()?;
                        let old_word = builder.build_load(i64_type, elem_ptr, "old_word").ok()?.into_int_value();
                        let new_word = builder.build_and(old_word, not_mask, "new_word").ok()?;
                        let _ = builder.build_store(elem_ptr, new_word);
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), word_idx.into()], "arr_get_call").ok()?;
                        let old_word = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        };
                        let new_word = builder.build_and(old_word, not_mask, "new_word").ok()?;
                        let _ = builder.build_call(arr_set_int_fn, &[arr_ptr.into(), word_idx.into(), new_word.into()], "arr_set_call").ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::IBitSet => {
                let bit_idx = resolve_int(&builder, &ops[1]).ok()?;
                let val = resolve_int(&builder, &ops[2]).ok()?;
                let const_6 = i64_type.const_int(6, false);
                let const_63 = i64_type.const_int(63, false);
                let const_1 = i64_type.const_int(1, false);
                let const_0 = i64_type.const_int(0, false);

                let word_idx = builder.build_right_shift(bit_idx, const_6, false, "bit_word_idx").ok()?;
                let bit_pos = builder.build_and(bit_idx, const_63, "bit_pos").ok()?;
                let mask = builder.build_left_shift(const_1, bit_pos, "bit_mask").ok()?;
                let not_mask = builder.build_not(mask, "not_mask").ok()?;
                let is_nonzero = builder.build_int_compare(IntPredicate::NE, val, const_0, "is_nonzero").ok()?;

                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[word_idx], "word_ptr") }.ok()?;
                        let old_word = builder.build_load(i64_type, elem_ptr, "old_word").ok()?.into_int_value();
                        let set1_word = builder.build_or(old_word, mask, "set1_word").ok()?;
                        let set0_word = builder.build_and(old_word, not_mask, "set0_word").ok()?;
                        let new_word = builder.build_select(is_nonzero, set1_word, set0_word, "new_word").ok()?.into_int_value();
                        let _ = builder.build_store(elem_ptr, new_word);
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), word_idx.into()], "arr_get_call").ok()?;
                        let old_word = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        };
                        let set1_word = builder.build_or(old_word, mask, "set1_word").ok()?;
                        let set0_word = builder.build_and(old_word, not_mask, "set0_word").ok()?;
                        let new_word = builder.build_select(is_nonzero, set1_word, set0_word, "new_word").ok()?.into_int_value();
                        let _ = builder.build_call(arr_set_int_fn, &[arr_ptr.into(), word_idx.into(), new_word.into()], "arr_set_call").ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FArrayGet => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                match ops[0] {
                    OperandType::Argument(a) if arg_float_ptrs.contains_key(&a) => {
                        let data_ptr = arg_float_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(f64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                        let direct_val = builder.build_load(f64_type, elem_ptr, "direct_val").ok()?.into_float_value();
                        if let OperandType::Register(dst) = ops[2] {
                            set_reg_float(&builder, dst, direct_val).ok()?;
                        }
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_flt_fn, &[arr_ptr.into(), idx_val.into()], "arr_get_flt_call").ok()?;
                        let res_val = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_float_value(),
                            _ => return None,
                        };
                        if let OperandType::Register(dst) = ops[2] {
                            set_reg_float(&builder, dst, res_val).ok()?;
                        }
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::FArraySet => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                let val_to_store = resolve_float(&builder, &ops[2]).ok()?;
                match ops[0] {
                    OperandType::Argument(a) if arg_float_ptrs.contains_key(&a) => {
                        let data_ptr = arg_float_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(f64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                        let _ = builder.build_store(elem_ptr, val_to_store);
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let _ = builder.build_call(arr_set_flt_fn, &[arr_ptr.into(), idx_val.into(), val_to_store.into()], "arr_set_flt_call").ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::ArrayGet | Opcode::AGetIndexAR => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let is_null = builder.build_is_null(data_ptr, "data_is_null").ok()?;
                        let direct_bb = context.append_basic_block(llvm_fn, &format!("arr_direct_{}", i));
                        let obj_bb = context.append_basic_block(llvm_fn, &format!("arr_obj_{}", i));
                        let merge_bb = context.append_basic_block(llvm_fn, &format!("arr_merge_{}", i));

                        let _ = builder.build_conditional_branch(is_null, obj_bb, direct_bb);

                        builder.position_at_end(direct_bb);
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                        let direct_val = builder.build_load(i64_type, elem_ptr, "direct_val").ok()?.into_int_value();
                        let _ = builder.build_unconditional_branch(merge_bb);

                        builder.position_at_end(obj_bb);
                        let arg_ptr = get_arg_ptr(&builder, a).ok()?;
                        let call_res = builder.build_call(helper_fn, &[arg_ptr.into(), idx_val.into()], "get_prop_call").ok()?;
                        let obj_val = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        };
                        let _ = builder.build_unconditional_branch(merge_bb);

                        builder.position_at_end(merge_bb);
                        let phi = builder.build_phi(i64_type, "res_val").ok()?;
                        phi.add_incoming(&[(&direct_val, direct_bb), (&obj_val, obj_bb)]);
                        if let OperandType::Register(dst) = ops[2] {
                            set_reg_int(&builder, dst, phi.as_basic_value().into_int_value()).ok()?;
                        }
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                            OperandType::This => self_param,
                            _ => get_arg_ptr(&builder, 0).ok()?,
                        };
                        let call_res = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), idx_val.into()], "arr_get_call").ok()?;
                        let res_val = match call_res.try_as_basic_value() {
                            inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                            _ => return None,
                        };
                        if let OperandType::Register(dst) = ops[2] {
                            set_reg_int(&builder, dst, res_val).ok()?;
                        }
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::GetIndex => {
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                let arg_idx = match ops[0] {
                    OperandType::Argument(a) => Some(a),
                    OperandType::Register(r) => reg_to_arg.get(&r).copied(),
                    _ => None,
                };
                if let Some(a) = arg_idx.filter(|a| arg_int_ptrs.contains_key(a)) {
                    let data_ptr = arg_int_ptrs[&a];
                    let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                    let direct_val = builder.build_load(i64_type, elem_ptr, "direct_val").ok()?.into_int_value();
                    if let OperandType::Register(dst) = ops[2] {
                        set_reg_int(&builder, dst, direct_val).ok()?;
                    }
                } else {
                    let obj_ptr = match ops[0] {
                        OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                        OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                        OperandType::This => self_param,
                        _ => get_arg_ptr(&builder, 0).ok()?,
                    };
                    let call_res = builder.build_call(helper_fn, &[obj_ptr.into(), idx_val.into()], "get_prop_call").ok()?;
                    let res_val = match call_res.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    if let OperandType::Register(dst) = ops[2] {
                        set_reg_int(&builder, dst, res_val).ok()?;
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Ret => {
                if !ops.is_empty() {
                    match &ops[0] {
                        OperandType::Constant(value @ Value::String(_)) => {
                            let source_ptr = i64_type
                                .const_int(value as *const Value as u64, false)
                                .const_to_pointer(ptr_type);
                            let _ = builder.build_call(
                                copy_value_fn,
                                &[out_res_param.into(), source_ptr.into()],
                                "copy_string_return",
                            );
                        }
                        OperandType::Constant(Value::Bool(b)) => {
                            let tag = i8_type.const_int(2, false);
                            let _ = builder.build_store(out_res_param, tag);
                            let bool_offset = i64_type.const_int(8, false);
                            if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[bool_offset], "out_val_ptr") } {
                                let val = i64_type.const_int(if *b { 1 } else { 0 }, false);
                                let _ = builder.build_store(val_ptr, val);
                            }
                        }
                        OperandType::Constant(Value::Int(c)) => {
                            let tag = i8_type.const_int(0, false);
                            let _ = builder.build_store(out_res_param, tag);
                            let int_offset = i64_type.const_int(8, false);
                            if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[int_offset], "out_val_ptr") } {
                                let val = i64_type.const_int(*c as u64, true);
                                let _ = builder.build_store(val_ptr, val);
                            }
                        }
                        OperandType::Constant(Value::Float(f)) => {
                            let tag = i8_type.const_int(1, false);
                            let _ = builder.build_store(out_res_param, tag);
                            let flt_offset = i64_type.const_int(8, false);
                            if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[flt_offset], "out_val_ptr") } {
                                let val = f64_type.const_float(*f);
                                let _ = builder.build_store(val_ptr, val);
                            }
                        }
                        OperandType::Constant(Value::Nil) => {
                            let tag = i8_type.const_int(6, false);
                            let _ = builder.build_store(out_res_param, tag);
                        }
                        OperandType::Register(r) => {
                            let reg_ptr = get_reg_ptr(&builder, *r).ok()?;
                            if *r < reg_val_allocas.len() {
                                let tag = builder.build_load(i8_type, reg_tag_allocas[*r], "ret_tag").ok()?.into_int_value();
                                let scalar_ret_bb = context.append_basic_block(llvm_fn, "scalar_ret");
                                let heap_ret_bb = context.append_basic_block(llvm_fn, "heap_ret");
                                let ret_cont_bb = context.append_basic_block(llvm_fn, "ret_cont");

                                if has_vector_ops {
                                    let v4i_ret_bb = context.append_basic_block(llvm_fn, "v4i_ret");
                                    let v4f_ret_bb = context.append_basic_block(llvm_fn, "v4f_ret");

                                    let _ = builder.build_switch(
                                        tag,
                                        heap_ret_bb,
                                        &[
                                            (i8_type.const_int(0, false), scalar_ret_bb),
                                            (i8_type.const_int(1, false), scalar_ret_bb),
                                            (i8_type.const_int(2, false), scalar_ret_bb),
                                            (i8_type.const_int(6, false), scalar_ret_bb),
                                            (i8_type.const_int(10, false), v4i_ret_bb),
                                            (i8_type.const_int(11, false), v4f_ret_bb),
                                        ],
                                    );

                                    // v4i_ret_bb:
                                    builder.position_at_end(v4i_ret_bb);
                                    let _ = builder.build_store(out_res_param, tag);
                                    let val_offset = i64_type.const_int(8, false);
                                    if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[val_offset], "out_v4i_ptr") } {
                                        if let Ok(v4i_val) = builder.build_load(v4i64_type, reg_v4i_allocas[*r], "ret_v4i") {
                                            let _ = builder.build_store(val_ptr, v4i_val);
                                        }
                                    }
                                    let _ = builder.build_unconditional_branch(ret_cont_bb);

                                    // v4f_ret_bb:
                                    builder.position_at_end(v4f_ret_bb);
                                    let _ = builder.build_store(out_res_param, tag);
                                    if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[val_offset], "out_v4f_ptr") } {
                                        if let Ok(v4f_val) = builder.build_load(v4f64_type, reg_v4f_allocas[*r], "ret_v4f") {
                                            let _ = builder.build_store(val_ptr, v4f_val);
                                        }
                                    }
                                    let _ = builder.build_unconditional_branch(ret_cont_bb);
                                } else {
                                    let _ = builder.build_switch(
                                        tag,
                                        heap_ret_bb,
                                        &[
                                            (i8_type.const_int(0, false), scalar_ret_bb),
                                            (i8_type.const_int(1, false), scalar_ret_bb),
                                            (i8_type.const_int(2, false), scalar_ret_bb),
                                            (i8_type.const_int(6, false), scalar_ret_bb),
                                        ],
                                    );
                                }

                                // scalar_ret_bb:
                                builder.position_at_end(scalar_ret_bb);
                                let _ = builder.build_store(out_res_param, tag);
                                let val_offset = i64_type.const_int(8, false);
                                if let Ok(val_ptr) = unsafe { builder.build_gep(i8_type, out_res_param, &[val_offset], "out_val_ptr") } {
                                    if let Ok(val) = builder.build_load(i64_type, reg_val_allocas[*r], "ret_val") {
                                        let _ = builder.build_store(val_ptr, val);
                                    }
                                }
                                let _ = builder.build_unconditional_branch(ret_cont_bb);

                                // heap_ret_bb:
                                builder.position_at_end(heap_ret_bb);
                                let _ = builder.build_call(
                                    copy_value_fn,
                                    &[out_res_param.into(), reg_ptr.into()],
                                    "copy_heap_register_return",
                                );
                                let _ = builder.build_unconditional_branch(ret_cont_bb);

                                builder.position_at_end(ret_cont_bb);
                            } else {
                                let _ = builder.build_call(
                                    copy_value_fn,
                                    &[out_res_param.into(), reg_ptr.into()],
                                    "copy_register_return",
                                );
                            }
                        }
                        OperandType::Captured(c) => {
                            if let Ok(cap_ptr) = get_cap_ptr(&builder, *c) {
                                let _ = builder.build_call(
                                    copy_value_fn,
                                    &[out_res_param.into(), cap_ptr.into()],
                                    "copy_capture_return",
                                );
                            }
                        }
                        OperandType::Argument(a) => {
                            if let Ok(arg_ptr) = get_arg_ptr(&builder, *a) {
                                let _ = builder.build_call(
                                    copy_value_fn,
                                    &[out_res_param.into(), arg_ptr.into()],
                                    "copy_argument_return",
                                );
                            }
                        }
                        OperandType::This => {
                            let _ = builder.build_call(
                                copy_value_fn,
                                &[out_res_param.into(), self_param.into()],
                                "copy_this_return",
                            );
                        }
                        _ => {}
                    }
                }
                let _ = builder.build_return(None);
            }
            Opcode::Nop => {
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                }
            }
            Opcode::ETagJmpRC => {
                let obj_ptr = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    OperandType::This => self_param,
                    _ => get_arg_ptr(&builder, 0).ok()?,
                };
                let expected_tag = match ops[1] {
                    OperandType::Constant(Value::Int(tag)) => tag,
                    _ => -1,
                };
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };

                let call_res = builder.build_call(get_enum_tag_fn, &[obj_ptr.into()], "enum_tag_call").ok()?;
                let tag_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };
                let exp_val = i64_type.const_int(expected_tag as u64, true);
                let cmp = builder.build_int_compare(IntPredicate::EQ, tag_val, exp_val, "tag_cmp").ok()?;
                let _ = builder.build_conditional_branch(cmp, target_bb, next_bb);
            }
            Opcode::ETagSwitch => {
                let obj_ptr = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    OperandType::This => self_param,
                    _ => get_arg_ptr(&builder, 0).ok()?,
                };
                let table = match &ops[1] {
                    OperandType::JumpTable(t) => t.clone(),
                    _ => vec![],
                };
                let default_off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let def_idx = (i as i64 + default_off) as usize;
                let default_bb = *bbs.get(def_idx)?;

                let call_res = builder.build_call(get_enum_tag_fn, &[obj_ptr.into()], "enum_tag_call").ok()?;
                let tag_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };

                let mut switch_cases = Vec::with_capacity(table.len());
                for case in &table {
                    let case_val = i64_type.const_int(case.tag as u64, true);
                    let target_idx = (i as i64 + case.offset) as usize;
                    let target_bb = *bbs.get(target_idx)?;
                    switch_cases.push((case_val, target_bb));
                }
                let _ = builder.build_switch(tag_val, default_bb, &switch_cases);
            }
            Opcode::AMatchFast => {
                let regs = match &ops[0] {
                    OperandType::RegisterList(regs) => regs.clone(),
                    _ => vec![],
                };
                let consts = match &ops[1] {
                    OperandType::ConstantList(consts) => consts.clone(),
                    _ => vec![],
                };
                let off = match ops[2] {
                    OperandType::Offset(o) => o,
                    _ => 1,
                };
                let target_idx = (i as i64 + off) as usize;
                let target_bb = *bbs.get(target_idx)?;
                let next_bb = if i + 1 < inst_count { bbs[i + 1] } else { target_bb };

                let mut matched_cond: Option<IntValue<'static>> = None;
                for (r, c) in regs.iter().zip(consts.iter()) {
                    let reg_val = get_reg_int(&builder, *r).ok()?;
                    let c_int = match c {
                        Value::Int(val) => *val,
                        _ => 0,
                    };
                    let c_val = i64_type.const_int(c_int as u64, true);
                    let eq = builder.build_int_compare(IntPredicate::EQ, reg_val, c_val, "fast_match_eq").ok()?;
                    matched_cond = match matched_cond {
                        Some(prev) => Some(builder.build_and(prev, eq, "and_match").ok()?),
                        None => Some(eq),
                    };
                }
                let all_matched = matched_cond.unwrap_or_else(|| i8_type.const_int(1, false).into());
                let _ = builder.build_conditional_branch(all_matched, next_bb, target_bb);
            }
            Opcode::Match => {
                let mut cases: Vec<(Pattern, i64)> = Vec::new();
                let mut default_off: Option<i64> = None;
                let mut p_idx = 1;
                while p_idx < ops.len() {
                    if p_idx + 1 < ops.len() {
                        if let OperandType::Pattern(pat) = &ops[p_idx] {
                            let off = match ops[p_idx + 1] {
                                OperandType::Offset(o) => o,
                                OperandType::Constant(Value::Int(o)) => o,
                                _ => 1,
                            };
                            cases.push((pat.clone(), off));
                            p_idx += 2;
                            continue;
                        }
                    }
                    if let OperandType::Offset(o) = ops[p_idx] {
                        default_off = Some(o);
                    } else if let OperandType::Constant(Value::Int(o)) = ops[p_idx] {
                        default_off = Some(o);
                    }
                    p_idx += 1;
                }

                let default_target_bb = if let Some(off) = default_off {
                    let def_idx = (i as i64 + off) as usize;
                    *bbs.get(def_idx)?
                } else if i + 1 < inst_count {
                    bbs[i + 1]
                } else {
                    bbs[i]
                };

                let is_all_int = cases.iter().all(|(pat, _)| match pat {
                    Pattern::Literal(Value::Int(_))
                    | Pattern::IntRange(_, _, _)
                    | Pattern::Bind(_)
                    | Pattern::Wildcard => true,
                    _ => false,
                });

                let is_all_float = cases.iter().all(|(pat, _)| match pat {
                    Pattern::Literal(Value::Float(_))
                    | Pattern::FloatRange(_, _, _)
                    | Pattern::Bind(_)
                    | Pattern::Wildcard => true,
                    _ => false,
                });

                if is_all_int && !cases.is_empty() {
                    let val = resolve_int(&builder, &ops[0]).ok()?;
                    let is_pure_switch = cases.iter().all(|(pat, _)| matches!(pat, Pattern::Literal(Value::Int(_))));
                    if is_pure_switch && cases.len() >= 2 {
                        let mut switch_cases = Vec::with_capacity(cases.len());
                        for (pat, off) in &cases {
                            if let Pattern::Literal(Value::Int(c)) = pat {
                                let target_idx = (i as i64 + off) as usize;
                                let target_bb = *bbs.get(target_idx)?;
                                switch_cases.push((i64_type.const_int(*c as u64, true), target_bb));
                            }
                        }
                        let _ = builder.build_switch(val, default_target_bb, &switch_cases);
                    } else {
                        for k in 0..cases.len() {
                            let (pat, off) = &cases[k];
                            let target_idx = (i as i64 + off) as usize;
                            let target_bb = *bbs.get(target_idx)?;
                            let is_last = k + 1 == cases.len();
                            let next_case_bb = if is_last {
                                default_target_bb
                            } else {
                                context.append_basic_block(llvm_fn, &format!("match_int_{}_{}", i, k + 1))
                            };

                            match pat {
                                Pattern::Wildcard => {
                                    let _ = builder.build_unconditional_branch(target_bb);
                                    break;
                                }
                                Pattern::Bind(dst_reg) => {
                                    set_reg_int(&builder, *dst_reg, val).ok()?;
                                    let _ = builder.build_unconditional_branch(target_bb);
                                    break;
                                }
                                Pattern::Literal(Value::Int(c)) => {
                                    let c_val = i64_type.const_int(*c as u64, true);
                                    let cmp = builder.build_int_compare(IntPredicate::EQ, val, c_val, "lit_eq").ok()?;
                                    let _ = builder.build_conditional_branch(cmp, target_bb, next_case_bb);
                                }
                                Pattern::IntRange(start, end, inclusive) => {
                                    let cond = match (start, end) {
                                        (Some(s), Some(e)) => {
                                            let s_val = i64_type.const_int(*s as u64, true);
                                            let e_val = i64_type.const_int(*e as u64, true);
                                            let cmp_s = builder.build_int_compare(IntPredicate::SGE, val, s_val, "rng_s").ok()?;
                                            let pred = if *inclusive { IntPredicate::SLE } else { IntPredicate::SLT };
                                            let cmp_e = builder.build_int_compare(pred, val, e_val, "rng_e").ok()?;
                                            builder.build_and(cmp_s, cmp_e, "rng_and").ok()?
                                        }
                                        (Some(s), None) => {
                                            let s_val = i64_type.const_int(*s as u64, true);
                                            builder.build_int_compare(IntPredicate::SGE, val, s_val, "rng_ge").ok()?
                                        }
                                        (None, Some(e)) => {
                                            let e_val = i64_type.const_int(*e as u64, true);
                                            let pred = if *inclusive { IntPredicate::SLE } else { IntPredicate::SLT };
                                            builder.build_int_compare(pred, val, e_val, "rng_le").ok()?
                                        }
                                        (None, None) => {
                                            let _ = builder.build_unconditional_branch(target_bb);
                                            break;
                                        }
                                    };
                                    let _ = builder.build_conditional_branch(cond, target_bb, next_case_bb);
                                }
                                _ => return None,
                            }
                            if !is_last {
                                builder.position_at_end(next_case_bb);
                            }
                        }
                    }
                } else if is_all_float && !cases.is_empty() {
                    let val = resolve_float(&builder, &ops[0]).ok()?;
                    for k in 0..cases.len() {
                        let (pat, off) = &cases[k];
                        let target_idx = (i as i64 + off) as usize;
                        let target_bb = *bbs.get(target_idx)?;
                        let is_last = k + 1 == cases.len();
                        let next_case_bb = if is_last {
                            default_target_bb
                        } else {
                            context.append_basic_block(llvm_fn, &format!("match_flt_{}_{}", i, k + 1))
                        };

                        match pat {
                            Pattern::Wildcard => {
                                let _ = builder.build_unconditional_branch(target_bb);
                                break;
                            }
                            Pattern::Bind(dst_reg) => {
                                set_reg_float(&builder, *dst_reg, val).ok()?;
                                let _ = builder.build_unconditional_branch(target_bb);
                                break;
                            }
                            Pattern::Literal(Value::Float(f)) => {
                                let f_val = f64_type.const_float(*f);
                                let cmp = builder.build_float_compare(FloatPredicate::OEQ, val, f_val, "flt_eq").ok()?;
                                let _ = builder.build_conditional_branch(cmp, target_bb, next_case_bb);
                            }
                            Pattern::FloatRange(start, end, inclusive) => {
                                let cond = match (start, end) {
                                    (Some(s), Some(e)) => {
                                        let s_val = f64_type.const_float(*s);
                                        let e_val = f64_type.const_float(*e);
                                        let cmp_s = builder.build_float_compare(FloatPredicate::OGE, val, s_val, "flt_rng_s").ok()?;
                                        let pred = if *inclusive { FloatPredicate::OLE } else { FloatPredicate::OLT };
                                        let cmp_e = builder.build_float_compare(pred, val, e_val, "flt_rng_e").ok()?;
                                        builder.build_and(cmp_s, cmp_e, "flt_rng_and").ok()?
                                    }
                                    (Some(s), None) => {
                                        let s_val = f64_type.const_float(*s);
                                        builder.build_float_compare(FloatPredicate::OGE, val, s_val, "flt_rng_ge").ok()?
                                    }
                                    (None, Some(e)) => {
                                        let e_val = f64_type.const_float(*e);
                                        let pred = if *inclusive { FloatPredicate::OLE } else { FloatPredicate::OLT };
                                        builder.build_float_compare(pred, val, e_val, "flt_rng_le").ok()?
                                    }
                                    (None, None) => {
                                        let _ = builder.build_unconditional_branch(target_bb);
                                        break;
                                    }
                                };
                                let _ = builder.build_conditional_branch(cond, target_bb, next_case_bb);
                            }
                            _ => return None,
                        }
                        if !is_last {
                            builder.position_at_end(next_case_bb);
                        }
                    }
                } else if !cases.is_empty() {
                    let is_all_fixed_array = cases.iter().all(|(pat, _)| match pat {
                        Pattern::IntArray(arr_pat) | Pattern::Array(arr_pat) => {
                            arr_pat.segments.iter().all(|seg| matches!(seg, ListSegment::Element(_)))
                        }
                        Pattern::Wildcard => true,
                        _ => false,
                    });

                    let data_ptr_opt = match ops[0] {
                        OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => Some(arg_int_ptrs[&a]),
                        _ => None,
                    };

                    if is_all_fixed_array && data_ptr_opt.is_some() {
                        let data_ptr = data_ptr_opt.unwrap();
                        for k in 0..cases.len() {
                            let (pat, off) = &cases[k];
                            let target_idx = (i as i64 + off) as usize;
                            let target_bb = *bbs.get(target_idx)?;
                            let is_last = k + 1 == cases.len();
                            let next_case_bb = if is_last {
                                default_target_bb
                            } else {
                                context.append_basic_block(llvm_fn, &format!("match_arr_{}_{}", i, k + 1))
                            };

                            match pat {
                                Pattern::Wildcard => {
                                    let _ = builder.build_unconditional_branch(target_bb);
                                    break;
                                }
                                Pattern::IntArray(arr_pat) | Pattern::Array(arr_pat) => {
                                    let mut all_elem_cond: Option<IntValue<'static>> = None;
                                    for (elem_idx, seg) in arr_pat.segments.iter().enumerate() {
                                        if let ListSegment::Element(elem_pat) = seg {
                                            let idx_val = i64_type.const_int(elem_idx as u64, false);
                                            let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "arr_elem_ptr") }.ok()?;
                                            let elem_val = builder.build_load(i64_type, elem_ptr, "arr_elem").ok()?.into_int_value();

                                            let elem_cond = match elem_pat {
                                                Pattern::Literal(Value::Int(c)) => {
                                                    let c_val = i64_type.const_int(*c as u64, true);
                                                    builder.build_int_compare(IntPredicate::EQ, elem_val, c_val, "elem_eq").ok()?
                                                }
                                                Pattern::IntRange(start, end, inclusive) => {
                                                    match (start, end) {
                                                        (Some(s), Some(e)) => {
                                                            let s_val = i64_type.const_int(*s as u64, true);
                                                            let e_val = i64_type.const_int(*e as u64, true);
                                                            let cmp_s = builder.build_int_compare(IntPredicate::SGE, elem_val, s_val, "elem_rng_s").ok()?;
                                                            let pred = if *inclusive { IntPredicate::SLE } else { IntPredicate::SLT };
                                                            let cmp_e = builder.build_int_compare(pred, elem_val, e_val, "elem_rng_e").ok()?;
                                                            builder.build_and(cmp_s, cmp_e, "elem_rng_and").ok()?
                                                        }
                                                        (Some(s), None) => {
                                                            let s_val = i64_type.const_int(*s as u64, true);
                                                            builder.build_int_compare(IntPredicate::SGE, elem_val, s_val, "elem_rng_ge").ok()?
                                                        }
                                                        (None, Some(e)) => {
                                                            let e_val = i64_type.const_int(*e as u64, true);
                                                            let pred = if *inclusive { IntPredicate::SLE } else { IntPredicate::SLT };
                                                            builder.build_int_compare(pred, elem_val, e_val, "elem_rng_le").ok()?
                                                        }
                                                        (None, None) => i8_type.const_int(1, false).into(),
                                                    }
                                                }
                                                Pattern::Bind(dst_reg) => {
                                                    set_reg_int(&builder, *dst_reg, elem_val).ok()?;
                                                    i8_type.const_int(1, false).into()
                                                }
                                                Pattern::Wildcard => i8_type.const_int(1, false).into(),
                                                _ => return None,
                                            };

                                            all_elem_cond = match all_elem_cond {
                                                Some(prev) => Some(builder.build_and(prev, elem_cond, "arr_and").ok()?),
                                                None => Some(elem_cond),
                                            };
                                        }
                                    }
                                    let final_cond = all_elem_cond.unwrap_or_else(|| i8_type.const_int(1, false).into());
                                    let _ = builder.build_conditional_branch(final_cond, target_bb, next_case_bb);
                                }
                                _ => return None,
                            }
                            if !is_last {
                                builder.position_at_end(next_case_bb);
                            }
                        }
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
            Opcode::VIotaI => {
                let start_val = resolve_int(&builder, &ops[0]).ok()?;
                let c0 = start_val;
                let c1 = builder.build_int_add(c0, i64_type.const_int(1, false), "iota1").ok()?;
                let c2 = builder.build_int_add(c0, i64_type.const_int(2, false), "iota2").ok()?;
                let c3 = builder.build_int_add(c0, i64_type.const_int(3, false), "iota3").ok()?;
                let undef = v4i64_type.get_undef();
                let v0 = builder.build_insert_element(undef, c0, i32_type.const_int(0, false), "v0").ok()?;
                let v1 = builder.build_insert_element(v0, c1, i32_type.const_int(1, false), "v1").ok()?;
                let v2 = builder.build_insert_element(v1, c2, i32_type.const_int(2, false), "v2").ok()?;
                let v3 = builder.build_insert_element(v2, c3, i32_type.const_int(3, false), "v3").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, v3).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSplatI => {
                let val = resolve_int(&builder, &ops[0]).ok()?;
                let undef = v4i64_type.get_undef();
                let v0 = builder.build_insert_element(undef, val, i32_type.const_int(0, false), "s0").ok()?;
                let v1 = builder.build_insert_element(v0, val, i32_type.const_int(1, false), "s1").ok()?;
                let v2 = builder.build_insert_element(v1, val, i32_type.const_int(2, false), "s2").ok()?;
                let v3 = builder.build_insert_element(v2, val, i32_type.const_int(3, false), "s3").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, v3).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSplatF => {
                let val = resolve_float(&builder, &ops[0]).ok()?;
                let undef = v4f64_type.get_undef();
                let v0 = builder.build_insert_element(undef, val, i32_type.const_int(0, false), "sf0").ok()?;
                let v1 = builder.build_insert_element(v0, val, i32_type.const_int(1, false), "sf1").ok()?;
                let v2 = builder.build_insert_element(v1, val, i32_type.const_int(2, false), "sf2").ok()?;
                let v3 = builder.build_insert_element(v2, val, i32_type.const_int(3, false), "sf3").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4f(&builder, dst, v3).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VAddI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_int_add(v1, v2, "vadd_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSubI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_int_sub(v1, v2, "vsub_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VMulI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_int_mul(v1, v2, "vmul_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VAddIC => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let c_val = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i64_type.const_int(*c as u64, true),
                    _ => resolve_int(&builder, &ops[1]).ok()?,
                };
                let undef = v4i64_type.get_undef();
                let s0 = builder.build_insert_element(undef, c_val, i32_type.const_int(0, false), "c0").ok()?;
                let s1 = builder.build_insert_element(s0, c_val, i32_type.const_int(1, false), "c1").ok()?;
                let s2 = builder.build_insert_element(s1, c_val, i32_type.const_int(2, false), "c2").ok()?;
                let s3 = builder.build_insert_element(s2, c_val, i32_type.const_int(3, false), "c3").ok()?;
                let res = builder.build_int_add(v1, s3, "vaddic_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSubIC => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let c_val = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i64_type.const_int(*c as u64, true),
                    _ => resolve_int(&builder, &ops[1]).ok()?,
                };
                let undef = v4i64_type.get_undef();
                let s0 = builder.build_insert_element(undef, c_val, i32_type.const_int(0, false), "c0").ok()?;
                let s1 = builder.build_insert_element(s0, c_val, i32_type.const_int(1, false), "c1").ok()?;
                let s2 = builder.build_insert_element(s1, c_val, i32_type.const_int(2, false), "c2").ok()?;
                let s3 = builder.build_insert_element(s2, c_val, i32_type.const_int(3, false), "c3").ok()?;
                let res = builder.build_int_sub(v1, s3, "vsubic_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VMulIC => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let c_val = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i64_type.const_int(*c as u64, true),
                    _ => resolve_int(&builder, &ops[1]).ok()?,
                };
                let undef = v4i64_type.get_undef();
                let s0 = builder.build_insert_element(undef, c_val, i32_type.const_int(0, false), "c0").ok()?;
                let s1 = builder.build_insert_element(s0, c_val, i32_type.const_int(1, false), "c1").ok()?;
                let s2 = builder.build_insert_element(s1, c_val, i32_type.const_int(2, false), "c2").ok()?;
                let s3 = builder.build_insert_element(s2, c_val, i32_type.const_int(3, false), "c3").ok()?;
                let res = builder.build_int_mul(v1, s3, "vmulic_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VDivIC => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let c_val = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i64_type.const_int(*c as u64, true),
                    _ => resolve_int(&builder, &ops[1]).ok()?,
                };
                let undef = v4i64_type.get_undef();
                let s0 = builder.build_insert_element(undef, c_val, i32_type.const_int(0, false), "c0").ok()?;
                let s1 = builder.build_insert_element(s0, c_val, i32_type.const_int(1, false), "c1").ok()?;
                let s2 = builder.build_insert_element(s1, c_val, i32_type.const_int(2, false), "c2").ok()?;
                let s3 = builder.build_insert_element(s2, c_val, i32_type.const_int(3, false), "c3").ok()?;
                let res = builder.build_int_unsigned_div(v1, s3, "vdivic_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VRemIC => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let c_val = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i64_type.const_int(*c as u64, true),
                    _ => resolve_int(&builder, &ops[1]).ok()?,
                };
                let undef = v4i64_type.get_undef();
                let s0 = builder.build_insert_element(undef, c_val, i32_type.const_int(0, false), "c0").ok()?;
                let s1 = builder.build_insert_element(s0, c_val, i32_type.const_int(1, false), "c1").ok()?;
                let s2 = builder.build_insert_element(s1, c_val, i32_type.const_int(2, false), "c2").ok()?;
                let s3 = builder.build_insert_element(s2, c_val, i32_type.const_int(3, false), "c3").ok()?;
                let res = builder.build_int_unsigned_rem(v1, s3, "vremic_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VDivI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_int_unsigned_div(v1, v2, "vdivi_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VRemI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_int_unsigned_rem(v1, v2, "vremi_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VCmpEqI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let cmp = builder.build_int_compare(IntPredicate::EQ, v1, v2, "vcmpeq_tmp").ok()?;
                let res = builder.build_int_z_extend(cmp, v4i64_type, "vcmpeq_zext").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VCmpGtI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let cmp = builder.build_int_compare(IntPredicate::SGT, v1, v2, "vcmpgt_tmp").ok()?;
                let res = builder.build_int_z_extend(cmp, v4i64_type, "vcmpgt_zext").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VCmpLtI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let cmp = builder.build_int_compare(IntPredicate::SLT, v1, v2, "vcmplt_tmp").ok()?;
                let res = builder.build_int_z_extend(cmp, v4i64_type, "vcmplt_zext").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VExtractI => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let lane = match &ops[1] {
                    OperandType::Constant(Value::Int(c)) => i32_type.const_int(*c as u64, false),
                    _ => {
                        let lane_i64 = resolve_int(&builder, &ops[1]).ok()?;
                        builder.build_int_truncate(lane_i64, i32_type, "lane_trunc").ok()?
                    }
                };
                let extracted = builder.build_extract_element(v1, lane, "vextract_tmp").ok()?.into_int_value();
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, extracted).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VAddF => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_float_add(v1, v2, "vfadd_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4f(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSubF => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_float_sub(v1, v2, "vfsub_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4f(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VMulF => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_float_mul(v1, v2, "vfmul_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4f(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VDivF => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let res = builder.build_float_div(v1, v2, "vfdiv_tmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4f(&builder, dst, res).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VDotF => {
                let v1 = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let v2 = match ops[1] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let prod = builder.build_float_mul(v1, v2, "vfdot_mul").ok()?;
                let e0 = builder.build_extract_element(prod, i32_type.const_int(0, false), "d0").ok()?.into_float_value();
                let e1 = builder.build_extract_element(prod, i32_type.const_int(1, false), "d1").ok()?.into_float_value();
                let e2 = builder.build_extract_element(prod, i32_type.const_int(2, false), "d2").ok()?.into_float_value();
                let e3 = builder.build_extract_element(prod, i32_type.const_int(3, false), "d3").ok()?.into_float_value();
                let s01 = builder.build_float_add(e0, e1, "ds01").ok()?;
                let s23 = builder.build_float_add(e2, e3, "ds23").ok()?;
                let sum = builder.build_float_add(s01, s23, "dot_sum").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_float(&builder, dst, sum).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSumF => {
                let v = match ops[0] {
                    OperandType::Register(r) => get_reg_v4f(&builder, r).ok()?,
                    _ => return None,
                };
                let e0 = builder.build_extract_element(v, i32_type.const_int(0, false), "e0").ok()?.into_float_value();
                let e1 = builder.build_extract_element(v, i32_type.const_int(1, false), "e1").ok()?.into_float_value();
                let e2 = builder.build_extract_element(v, i32_type.const_int(2, false), "e2").ok()?.into_float_value();
                let e3 = builder.build_extract_element(v, i32_type.const_int(3, false), "e3").ok()?.into_float_value();
                let s01 = builder.build_float_add(e0, e1, "fs01").ok()?;
                let s23 = builder.build_float_add(e2, e3, "fs23").ok()?;
                let sum = builder.build_float_add(s01, s23, "sum_flt").ok()?;
                if let OperandType::Register(dst) = ops[1] {
                    set_reg_float(&builder, dst, sum).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VSumI => {
                let v = match ops[0] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let e0 = builder.build_extract_element(v, i32_type.const_int(0, false), "ie0").ok()?.into_int_value();
                let e1 = builder.build_extract_element(v, i32_type.const_int(1, false), "ie1").ok()?.into_int_value();
                let e2 = builder.build_extract_element(v, i32_type.const_int(2, false), "ie2").ok()?.into_int_value();
                let e3 = builder.build_extract_element(v, i32_type.const_int(3, false), "ie3").ok()?.into_int_value();
                let s01 = builder.build_int_add(e0, e1, "is01").ok()?;
                let s23 = builder.build_int_add(e2, e3, "is23").ok()?;
                let sum = builder.build_int_add(s01, s23, "sum_int").ok()?;
                if let OperandType::Register(dst) = ops[1] {
                    set_reg_int(&builder, dst, sum).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::VGatherI => {
                let idx_vec = match ops[1] {
                    OperandType::Register(r) => get_reg_v4i(&builder, r).ok()?,
                    _ => return None,
                };
                let i0 = builder.build_extract_element(idx_vec, i32_type.const_int(0, false), "gi0").ok()?.into_int_value();
                let i1 = builder.build_extract_element(idx_vec, i32_type.const_int(1, false), "gi1").ok()?.into_int_value();
                let i2 = builder.build_extract_element(idx_vec, i32_type.const_int(2, false), "gi2").ok()?.into_int_value();
                let i3 = builder.build_extract_element(idx_vec, i32_type.const_int(3, false), "gi3").ok()?.into_int_value();

                let arg_idx = match ops[0] {
                    OperandType::Argument(a) => Some(a),
                    OperandType::Register(r) => reg_to_arg.get(&r).copied(),
                    _ => None,
                };

                let (val0, val1, val2, val3) = if let Some(a) = arg_idx.filter(|a| arg_int_ptrs.contains_key(a)) {
                    let data_ptr = arg_int_ptrs[&a];
                    let p0 = unsafe { builder.build_gep(i64_type, data_ptr, &[i0], "gp0") }.ok()?;
                    let p1 = unsafe { builder.build_gep(i64_type, data_ptr, &[i1], "gp1") }.ok()?;
                    let p2 = unsafe { builder.build_gep(i64_type, data_ptr, &[i2], "gp2") }.ok()?;
                    let p3 = unsafe { builder.build_gep(i64_type, data_ptr, &[i3], "gp3") }.ok()?;
                    let v0 = builder.build_load(i64_type, p0, "gv0").ok()?.into_int_value();
                    let v1 = builder.build_load(i64_type, p1, "gv1").ok()?.into_int_value();
                    let v2 = builder.build_load(i64_type, p2, "gv2").ok()?.into_int_value();
                    let v3 = builder.build_load(i64_type, p3, "gv3").ok()?.into_int_value();
                    (v0, v1, v2, v3)
                } else {
                    let arr_ptr = match ops[0] {
                        OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                        OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                        OperandType::This => self_param,
                        _ => get_arg_ptr(&builder, 0).ok()?,
                    };
                    let call0 = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), i0.into()], "c0").ok()?;
                    let c0 = match call0.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    let call1 = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), i1.into()], "c1").ok()?;
                    let c1 = match call1.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    let call2 = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), i2.into()], "c2").ok()?;
                    let c2 = match call2.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    let call3 = builder.build_call(arr_get_int_fn, &[arr_ptr.into(), i3.into()], "c3").ok()?;
                    let c3 = match call3.try_as_basic_value() {
                        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                        _ => return None,
                    };
                    (c0, c1, c2, c3)
                };

                let undef = v4i64_type.get_undef();
                let r0 = builder.build_insert_element(undef, val0, i32_type.const_int(0, false), "gr0").ok()?;
                let r1 = builder.build_insert_element(r0, val1, i32_type.const_int(1, false), "gr1").ok()?;
                let r2 = builder.build_insert_element(r1, val2, i32_type.const_int(2, false), "gr2").ok()?;
                let r3 = builder.build_insert_element(r2, val3, i32_type.const_int(3, false), "gr3").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_v4i(&builder, dst, r3).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SEqRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let p1 = match ops[1] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let call_res = builder.build_call(str_eq_fn, &[p0.into(), p1.into()], "str_eq_call").ok()?;
                let res_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };
                let cmp = builder.build_int_compare(IntPredicate::NE, res_val, i64_type.const_int(0, false), "bool_cmp").ok()?;
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_bool(&builder, dst, cmp).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SConcatRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let p1 = match ops[1] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let dst_ptr = match ops[2] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    _ => return None,
                };
                builder.build_call(str_concat_fn, &[p0.into(), p1.into(), dst_ptr.into()], "str_concat_call").ok()?;
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SFindRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let p1 = match ops[1] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let call_res = builder.build_call(str_find_fn, &[p0.into(), p1.into()], "str_find_call").ok()?;
                let res_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res_val).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SCountByteRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let byte_val = resolve_int(&builder, &ops[1]).ok()?;
                let call_res = builder.build_call(str_count_byte_fn, &[p0.into(), byte_val.into()], "str_count_call").ok()?;
                let res_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };
                if let OperandType::Register(dst) = ops[2] {
                    set_reg_int(&builder, dst, res_val).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SToIntRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let call_res = builder.build_call(str_to_int_fn, &[p0.into()], "str_to_int_call").ok()?;
                let res_val = match call_res.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
                    _ => return None,
                };
                if let OperandType::Register(dst) = ops[1] {
                    set_reg_int(&builder, dst, res_val).ok()?;
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SUpperRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let dst_ptr = match ops[1] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    _ => return None,
                };
                builder.build_call(str_upper_fn, &[p0.into(), dst_ptr.into()], "str_upper_call").ok()?;
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SLowerRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let dst_ptr = match ops[1] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    _ => return None,
                };
                builder.build_call(str_lower_fn, &[p0.into(), dst_ptr.into()], "str_lower_call").ok()?;
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SSliceRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let start_val = resolve_int(&builder, &ops[1]).ok()?;
                let dst_ptr = match ops[2] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    _ => return None,
                };
                let minus_one = i64_type.const_int(!0, true);
                builder.build_call(str_slice_fn, &[p0.into(), start_val.into(), minus_one.into(), dst_ptr.into()], "str_slice_call").ok()?;
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::SSliceRRRR => {
                let p0 = match ops[0] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
                    _ => return None,
                };
                let start_val = resolve_int(&builder, &ops[1]).ok()?;
                let end_val = resolve_int(&builder, &ops[2]).ok()?;
                let dst_ptr = match ops[3] {
                    OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                    _ => return None,
                };
                builder.build_call(str_slice_fn, &[p0.into(), start_val.into(), end_val.into(), dst_ptr.into()], "str_slice_call").ok()?;
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            _ => {
                return None;
            }
        }
    }

    let triple = TargetMachine::get_default_triple();
    if let Some(target) = Target::from_triple(&triple).ok() {
        let cpu = TargetMachine::get_host_cpu_name().to_string();
        let features = TargetMachine::get_host_cpu_features().to_string();
        if let Some(tm) = target.create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Aggressive,
            RelocMode::Default,
            CodeModel::JITDefault,
        ) {
            let pass_opts = PassBuilderOptions::create();
            pass_opts.set_loop_vectorization(true);
            pass_opts.set_loop_slp_vectorization(true);
            pass_opts.set_loop_unrolling(true);
            pass_opts.set_loop_interleaving(true);
            let _ = module.run_passes("default<O3>,loop-vectorize,slp-vectorizer", &tm, pass_opts);
        }
    }

    let ir = module.print_to_string().to_string();
    unsafe {
        let compiled = ee.get_function::<NativeJitFn>(&fn_name).ok()?;
        Some((compiled.as_raw(), ir))
    }
}

pub fn compile_with_llvm(func: &Function) -> Option<NativeJitFn> {
    compile_with_llvm_ir(func).map(|(f, _)| f)
}
