use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::passes::PassBuilderOptions;
use inkwell::values::{FloatValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::{FloatPredicate, IntPredicate};
use inkwell::OptimizationLevel;
use std::sync::Mutex;

use crate::bytecode::instruction::OperandType;
use crate::bytecode::opcode::Opcode;
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

    let fn_type = void_type.fn_type(&[ptr_type.into(), ptr_type.into(), ptr_type.into()], false);
    let fn_name = format!("jit_{}", func.name);
    let llvm_fn = module.add_function(&fn_name, fn_type, None);

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

    let builder = context.create_builder();
    let entry_bb = context.append_basic_block(llvm_fn, "entry");

    let inst_count = func.instructions.len();
    let mut bbs = Vec::with_capacity(inst_count);
    for i in 0..inst_count {
        bbs.push(context.append_basic_block(llvm_fn, &format!("bb_{}", i)));
    }

    let regs_param = llvm_fn.get_nth_param(0)?.into_pointer_value();
    let args_param = llvm_fn.get_nth_param(1)?.into_pointer_value();
    let out_res_param = llvm_fn.get_nth_param(2)?.into_pointer_value();

    let val_size = std::mem::size_of::<Value>();

    let get_reg_ptr = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<PointerValue<'static>, inkwell::builder::BuilderError> {
        let offset = (reg_idx * val_size) as u64;
        let offset_val = i64_type.const_int(offset, false);
        unsafe { builder.build_gep(i8_type, regs_param, &[offset_val], "reg_ptr") }
    };

    let get_reg_int = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        let reg_ptr = get_reg_ptr(builder, reg_idx)?;
        let int_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[int_offset], "val_ptr") }?;
        let loaded = builder.build_load(i64_type, val_ptr, "reg_int")?;
        Ok(loaded.into_int_value())
    };

    let set_reg_int = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: IntValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let reg_ptr = get_reg_ptr(builder, reg_idx)?;
        let tag = i8_type.const_int(0, false);
        builder.build_store(reg_ptr, tag)?;
        let int_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[int_offset], "val_ptr") }?;
        builder.build_store(val_ptr, val)?;
        Ok(())
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
        let reg_ptr = get_reg_ptr(builder, reg_idx)?;
        let flt_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[flt_offset], "val_ptr") }?;
        let loaded = builder.build_load(f64_type, val_ptr, "reg_float")?;
        Ok(loaded.into_float_value())
    };

    let set_reg_float = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: FloatValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let reg_ptr = get_reg_ptr(builder, reg_idx)?;
        let tag = i8_type.const_int(1, false);
        builder.build_store(reg_ptr, tag)?;
        let flt_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[flt_offset], "val_ptr") }?;
        builder.build_store(val_ptr, val)?;
        Ok(())
    };

    let set_reg_bool = |builder: &inkwell::builder::Builder<'static>, reg_idx: usize, val: IntValue<'static>| -> Result<(), inkwell::builder::BuilderError> {
        let reg_ptr = get_reg_ptr(builder, reg_idx)?;
        let tag = i8_type.const_int(2, false);
        builder.build_store(reg_ptr, tag)?;
        let bool_offset = i64_type.const_int(8, false);
        let val_ptr = unsafe { builder.build_gep(i8_type, reg_ptr, &[bool_offset], "val_ptr") }?;
        let zext = builder.build_int_z_extend(val, i64_type, "bool_zext")?;
        builder.build_store(val_ptr, zext)?;
        Ok(())
    };

    let resolve_int = |builder: &inkwell::builder::Builder<'static>, op: &OperandType| -> Result<IntValue<'static>, inkwell::builder::BuilderError> {
        match op {
            OperandType::Register(r) => get_reg_int(builder, *r),
            OperandType::Argument(a) => get_arg_int(builder, *a),
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
            OperandType::Constant(Value::Float(c)) => Ok(f64_type.const_float(*c)),
            OperandType::Constant(Value::Int(c)) => Ok(f64_type.const_float(*c as f64)),
            _ => Ok(f64_type.const_float(0.0)),
        }
    };

    let mut int_array_args = std::collections::HashSet::new();
    let mut float_array_args = std::collections::HashSet::new();
    for inst in &func.instructions {
        match inst.opcode {
            Opcode::IArrayGet
            | Opcode::IArraySet
            | Opcode::IBitGet
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0 => {
                if let Some(OperandType::Argument(a)) = inst.operands.first() {
                    int_array_args.insert(*a);
                }
            }
            Opcode::FArrayGet | Opcode::FArraySet => {
                if let Some(OperandType::Argument(a)) = inst.operands.first() {
                    float_array_args.insert(*a);
                }
            }
            Opcode::ArrayGet | Opcode::AGetIndexAR => {
                if let Some(OperandType::Argument(a)) = inst.operands.first() {
                    int_array_args.insert(*a);
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
            Opcode::ISubRRR | Opcode::Sub => {
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
            Opcode::IMulRRR | Opcode::Mul => {
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
            Opcode::IDivRCR | Opcode::Div => {
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
            Opcode::IRemRCR | Opcode::Rem => {
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
                        OperandType::Argument(a) => {
                            let src_ptr = get_arg_ptr(&builder, *a).ok()?;
                            let dst_ptr = get_reg_ptr(&builder, dst).ok()?;
                            let src_tag = builder.build_load(i8_type, src_ptr, "src_tag").ok()?;
                            builder.build_store(dst_ptr, src_tag).ok()?;
                            let val_offset = i64_type.const_int(8, false);
                            let src_val_ptr = unsafe { builder.build_gep(i8_type, src_ptr, &[val_offset], "src_val_ptr") }.ok()?;
                            let dst_val_ptr = unsafe { builder.build_gep(i8_type, dst_ptr, &[val_offset], "dst_val_ptr") }.ok()?;
                            let val = builder.build_load(i64_type, src_val_ptr, "val").ok()?;
                            builder.build_store(dst_val_ptr, val).ok()?;
                        }
                        _ => {
                            let val = resolve_int(&builder, &ops[0]).ok()?;
                            set_reg_int(&builder, dst, val).ok()?;
                        }
                    }
                }
                if i + 1 < inst_count {
                    let _ = builder.build_unconditional_branch(bbs[i + 1]);
                } else {
                    let _ = builder.build_return(None);
                }
            }
            Opcode::Eq | Opcode::IEqRRR => {
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
            Opcode::Lt | Opcode::ILtRRR => {
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
            Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR => {
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
            Opcode::FAddRRR => {
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
            Opcode::FSubRRR => {
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
            Opcode::FMulRRR => {
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
            Opcode::FDivRRR => {
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
            Opcode::FEqRRR => {
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
            Opcode::FLtRRR => {
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
            Opcode::FGtRRR => {
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
                match ops[0] {
                    OperandType::Argument(a) if arg_int_ptrs.contains_key(&a) => {
                        let data_ptr = arg_int_ptrs[&a];
                        let elem_ptr = unsafe { builder.build_gep(i64_type, data_ptr, &[idx_val], "elem_ptr") }.ok()?;
                        let direct_val = builder.build_load(i64_type, elem_ptr, "direct_val").ok()?.into_int_value();
                        if let OperandType::Register(dst) = ops[2] {
                            set_reg_int(&builder, dst, direct_val).ok()?;
                        }
                    }
                    _ => {
                        let arr_ptr = match ops[0] {
                            OperandType::Register(r) => get_reg_ptr(&builder, r).ok()?,
                            OperandType::Argument(a) => get_arg_ptr(&builder, a).ok()?,
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
                let arg_idx = match ops[0] {
                    OperandType::Argument(a) => a,
                    _ => 0,
                };
                let obj_ptr = get_arg_ptr(&builder, arg_idx).ok()?;
                let idx_val = resolve_int(&builder, &ops[1]).ok()?;
                let call_res = builder.build_call(helper_fn, &[obj_ptr.into(), idx_val.into()], "get_prop_call").ok()?;
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
            Opcode::Ret => {
                if !ops.is_empty() {
                    match &ops[0] {
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
                            if let Ok(reg_ptr) = get_reg_ptr(&builder, *r) {
                                if let Ok(tag_val) = builder.build_load(i8_type, reg_ptr, "ret_tag") {
                                    let _ = builder.build_store(out_res_param, tag_val);
                                }
                                let payload_offset = i64_type.const_int(8, false);
                                if let (Ok(reg_payload_ptr), Ok(out_payload_ptr)) = unsafe {
                                    (builder.build_gep(i8_type, reg_ptr, &[payload_offset], "reg_payload_ptr"),
                                     builder.build_gep(i8_type, out_res_param, &[payload_offset], "out_payload_ptr"))
                                } {
                                    if let Ok(payload_val) = builder.build_load(i64_type, reg_payload_ptr, "ret_payload") {
                                        let _ = builder.build_store(out_payload_ptr, payload_val);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                let _ = builder.build_return(None);
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

