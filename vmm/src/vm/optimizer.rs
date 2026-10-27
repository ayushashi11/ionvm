use std::collections::HashMap;
use crate::bytecode::instruction::{Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, Value, ValType};

fn get_operand_type(op: &OperandType, reg_types: &[ValType], arg_types: &[ValType]) -> ValType {
    match op {
        OperandType::Register(r) => {
            if *r < reg_types.len() {
                reg_types[*r]
            } else {
                ValType::Unknown
            }
        }
        OperandType::Argument(a) => {
            if *a < arg_types.len() {
                arg_types[*a]
            } else {
                ValType::Unknown
            }
        }
        OperandType::Constant(c) => match c {
            Value::Int(_) => ValType::Int,
            Value::Float(_) => ValType::Float,
            Value::Bool(_) => ValType::Bool,
            Value::String(_) => ValType::String,
            Value::Object(_) => ValType::Object,
            Value::Array(_) => ValType::Array,
            Value::IntArray(_) => ValType::IntArray,
            Value::FloatArray(_) => ValType::FloatArray,
            Value::V4Int(_) => ValType::V4Int,
            Value::V4Float(_) => ValType::V4Float,
            Value::V4Bool(_) => ValType::V4Bool,
            _ => ValType::Unknown,
        },
        _ => ValType::Unknown,
    }
}

pub fn optimize_function(func: &mut Function) {
    let dummy_map = HashMap::new();
    optimize_function_with_types(func, &dummy_map);
}

pub fn optimize_bytecode(bytecode: &mut crate::bytecode::instruction::Bytecode) {
    let mut func_return_types = HashMap::new();
    for func in bytecode.functions.iter_mut() {
        optimize_function_with_types(func, &func_return_types);
        if let Some(ret) = func.return_type {
            func_return_types.insert(func.name.clone(), ret);
        }
    }
    for func in bytecode.functions.iter_mut() {
        optimize_function_with_types(func, &func_return_types);
        if let Some(ret) = func.return_type {
            func_return_types.insert(func.name.clone(), ret);
        }
    }
}

pub fn optimize_function_with_types(func: &mut Function, func_return_types: &HashMap<String, ValType>) {
    let mut reg_types = [ValType::Unknown; 64];
    let arg_types = [ValType::Unknown; 16];

    for _pass in 0..3 {
        for i in 0..func.instructions.len() {
            let inst = &mut func.instructions[i];
            let ops = &inst.operands;
            match inst.opcode {
                Opcode::LoadConst => {
                    if ops.len() >= 2 {
                        if let (OperandType::Constant(_), OperandType::Register(dst)) = (&ops[0], &ops[1]) {
                            if *dst < 64 {
                                reg_types[*dst] = get_operand_type(&ops[0], &reg_types, &arg_types);
                            }
                        } else if let (OperandType::Argument(_), OperandType::Register(dst)) = (&ops[0], &ops[1]) {
                            if *dst < 64 {
                                reg_types[*dst] = get_operand_type(&ops[0], &reg_types, &arg_types);
                            }
                        } else if let (OperandType::Register(src), OperandType::Register(dst)) = (&ops[0], &ops[1]) {
                            if *dst < 64 && *src < 64 {
                                reg_types[*dst] = reg_types[*src];
                            }
                        }
                    }
                }
                Opcode::Call | Opcode::CallJit => {
                    let arg_count = match ops.get(1) {
                        Some(OperandType::Offset(o)) => *o as usize,
                        _ => 0,
                    };
                    let dst_reg = match ops.get(2 + arg_count) {
                        Some(OperandType::Register(r)) => Some(*r),
                        _ => None,
                    };
                    if let Some(dst) = dst_reg {
                        let mut ret_type = ValType::Unknown;
                        if let Some(OperandType::Function(name)) = ops.first() {
                            if let Some(&known_ret) = func_return_types.get(name) {
                                ret_type = known_ret;
                            } else if let Some(native_ret) = crate::builtin::get_native_return_type(name) {
                                ret_type = native_ret;
                            }
                        }
                        if dst < 64 && ret_type != ValType::Unknown {
                            reg_types[dst] = ret_type;
                        }
                    }
                }
                Opcode::Add | Opcode::IAddRRR | Opcode::FAddRRR | Opcode::SConcatRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::FloatArray && t1 == ValType::FloatArray {
                                inst.opcode = Opcode::VAddF;
                                if dst < 64 {
                                    reg_types[dst] = ValType::FloatArray;
                                }
                            } else if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VAddI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::IntArray && (t1 == ValType::Int || matches!(ops[1], OperandType::Constant(Value::Int(_)))) {
                                inst.opcode = Opcode::VAddIC;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::String && t1 == ValType::String {
                                inst.opcode = Opcode::SConcatRRR;
                                if dst < 64 {
                                    reg_types[dst] = ValType::String;
                                }
                            } else if inst.opcode == Opcode::SConcatRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::String;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FAddRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::FAddRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IAddRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IAddRCR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if inst.opcode == Opcode::IAddRRR || inst.opcode == Opcode::IAddRCR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        }
                    }
                }
                Opcode::Sub | Opcode::ISubRRR | Opcode::FSubRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::FloatArray && t1 == ValType::FloatArray {
                                inst.opcode = Opcode::VSubF;
                                if dst < 64 {
                                    reg_types[dst] = ValType::FloatArray;
                                }
                            } else if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VSubI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::IntArray && (t1 == ValType::Int || matches!(ops[1], OperandType::Constant(Value::Int(_)))) {
                                inst.opcode = Opcode::VSubIC;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::ISubRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FSubRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::FSubRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::ISubRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        }
                    }
                }
                Opcode::Mul | Opcode::IMulRRR | Opcode::FMulRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::FloatArray && t1 == ValType::FloatArray {
                                inst.opcode = Opcode::VMulF;
                                if dst < 64 {
                                    reg_types[dst] = ValType::FloatArray;
                                }
                            } else if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VMulI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::IntArray && (t1 == ValType::Int || matches!(ops[1], OperandType::Constant(Value::Int(_)))) {
                                inst.opcode = Opcode::VMulIC;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IMulRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FMulRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::FMulRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::IMulRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        }
                    }
                }
                Opcode::Div | Opcode::IDivRCR | Opcode::FDivRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::FloatArray && t1 == ValType::FloatArray {
                                inst.opcode = Opcode::VDivF;
                                if dst < 64 {
                                    reg_types[dst] = ValType::FloatArray;
                                }
                            } else if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VDivI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::IntArray && (t1 == ValType::Int || matches!(ops[1], OperandType::Constant(Value::Int(_)))) {
                                inst.opcode = Opcode::VDivIC;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FDivRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if inst.opcode == Opcode::FDivRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IDivRCR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if inst.opcode == Opcode::IDivRCR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        }
                    }
                }
                Opcode::Rem | Opcode::IRemRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VRemI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::IntArray && (t1 == ValType::Int || matches!(ops[1], OperandType::Constant(Value::Int(_)))) {
                                inst.opcode = Opcode::VRemIC;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IRemRCR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            } else if inst.opcode == Opcode::IRemRCR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        }
                    }
                }
                Opcode::Shl | Opcode::IShlRRR | Opcode::IShlRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IShlRRR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IShlRCR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if inst.opcode == Opcode::IShlRRR || inst.opcode == Opcode::IShlRCR {
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            }
                        }
                    }
                }
                Opcode::Shr | Opcode::IShrRRR | Opcode::IShrRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IShrRRR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IShrRCR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if inst.opcode == Opcode::IShrRRR || inst.opcode == Opcode::IShrRCR {
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            }
                        }
                    }
                }
                Opcode::And | Opcode::IAndRRR | Opcode::IAndRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IAndRRR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IAndRCR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if inst.opcode == Opcode::IAndRRR || inst.opcode == Opcode::IAndRCR {
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Bool && t1 == ValType::Bool {
                                if dst < 64 { reg_types[dst] = ValType::Bool; }
                            }
                        }
                    }
                }
                Opcode::Or | Opcode::IOrRRR | Opcode::IOrRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IOrRRR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IOrRCR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if inst.opcode == Opcode::IOrRRR || inst.opcode == Opcode::IOrRCR {
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Bool && t1 == ValType::Bool {
                                if dst < 64 { reg_types[dst] = ValType::Bool; }
                            }
                        }
                    }
                }
                Opcode::Xor | Opcode::IXorRRR | Opcode::IXorRCR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IXorRRR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IXorRCR;
                                }
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if inst.opcode == Opcode::IXorRRR || inst.opcode == Opcode::IXorRCR {
                                if dst < 64 { reg_types[dst] = ValType::Int; }
                            } else if t0 == ValType::Bool && t1 == ValType::Bool {
                                if dst < 64 { reg_types[dst] = ValType::Bool; }
                            }
                        }
                    }
                }
                Opcode::Eq | Opcode::IEqRRR | Opcode::FEqRRR | Opcode::SEqRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VCmpEqI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::String && t1 == ValType::String {
                                inst.opcode = Opcode::SEqRRR;
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FEqRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::FEqRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IEqRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::IEqRRR || inst.opcode == Opcode::SEqRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            }
                        }
                    }
                }
                Opcode::Lt | Opcode::ILtRRR | Opcode::FLtRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VCmpLtI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FLtRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::FLtRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::ILtRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::ILtRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            }
                        }
                    }
                }
                Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::FGtRRR => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if let OperandType::Register(dst) = ops[2] {
                            if t0 == ValType::IntArray && t1 == ValType::IntArray {
                                inst.opcode = Opcode::VCmpGtI;
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            } else if t0 == ValType::Float && t1 == ValType::Float {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::FGtRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::FGtRRR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if t0 == ValType::Int && t1 == ValType::Int {
                                if matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IGtRRR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if t0 == ValType::Int && matches!(ops[1], OperandType::Constant(Value::Int(_))) {
                                if matches!(ops[0], OperandType::Register(_)) {
                                    inst.opcode = Opcode::IGtRCR;
                                }
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            } else if inst.opcode == Opcode::IGtRRR || inst.opcode == Opcode::IGtRCR {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Bool;
                                }
                            }
                        }
                    }
                }
                Opcode::JumpFalse | Opcode::IJumpFalseR => {
                    if ops.len() >= 2 && matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Offset(_)) {
                        inst.opcode = Opcode::IJumpFalseR;
                    }
                }
                Opcode::JumpTrue | Opcode::IJumpTrueR => {
                    if ops.len() >= 2 && matches!(ops[0], OperandType::Register(_)) && matches!(ops[1], OperandType::Offset(_)) {
                        inst.opcode = Opcode::IJumpTrueR;
                    }
                }
                Opcode::GetIndex | Opcode::AGetIndexAR | Opcode::ArrayGet | Opcode::IArrayGet | Opcode::FArrayGet => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t1 = get_operand_type(&ops[1], &reg_types, &arg_types);
                        if t0 == ValType::IntArray && t1 == ValType::IntArray {
                            inst.opcode = Opcode::VGatherI;
                            if let OperandType::Register(dst) = ops[2] {
                                if dst < 64 {
                                    reg_types[dst] = ValType::IntArray;
                                }
                            }
                        } else if t0 == ValType::IntArray {
                            inst.opcode = Opcode::IArrayGet;
                            if let OperandType::Register(dst) = ops[2] {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Int;
                                }
                            }
                        } else if t0 == ValType::FloatArray {
                            inst.opcode = Opcode::FArrayGet;
                            if let OperandType::Register(dst) = ops[2] {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Float;
                                }
                            }
                        } else if t0 == ValType::Array {
                            inst.opcode = Opcode::ArrayGet;
                            if let OperandType::Register(dst) = ops[2] {
                                if dst < 64 {
                                    reg_types[dst] = ValType::Unknown;
                                }
                            }
                        }
                    }
                }
                Opcode::ArraySet | Opcode::IArraySet | Opcode::FArraySet => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let t2 = get_operand_type(&ops[2], &reg_types, &arg_types);
                        if t0 == ValType::IntArray {
                            inst.opcode = Opcode::IArraySet;
                        } else if t0 == ValType::FloatArray || t2 == ValType::Float || matches!(ops[2], OperandType::Constant(Value::Float(_))) {
                            inst.opcode = Opcode::FArraySet;
                        }
                    }
                }
                Opcode::IBitGet => {
                    if let Some(OperandType::Register(dst)) = ops.get(2) {
                        if *dst < 64 {
                            reg_types[*dst] = ValType::Int;
                        }
                    }
                }
                Opcode::NewArray => {
                    if ops.len() >= 2 {
                        if let OperandType::Register(dst) = ops[1] {
                            if dst < 64 { reg_types[dst] = ValType::Array; }
                        }
                    } else if !ops.is_empty() {
                        if let OperandType::Register(dst) = ops[0] {
                            if dst < 64 { reg_types[dst] = ValType::Array; }
                        }
                    }
                }
                Opcode::NewIntArray => {
                    if ops.len() >= 2 {
                        if let OperandType::Register(dst) = ops[1] {
                            if dst < 64 { reg_types[dst] = ValType::IntArray; }
                        }
                    } else if !ops.is_empty() {
                        if let OperandType::Register(dst) = ops[0] {
                            if dst < 64 { reg_types[dst] = ValType::IntArray; }
                        }
                    }
                }
                Opcode::NewFloatArray => {
                    if ops.len() >= 2 {
                        if let OperandType::Register(dst) = ops[1] {
                            if dst < 64 { reg_types[dst] = ValType::FloatArray; }
                        }
                    } else if !ops.is_empty() {
                        if let OperandType::Register(dst) = ops[0] {
                            if dst < 64 { reg_types[dst] = ValType::FloatArray; }
                        }
                    }
                }
                Opcode::GetProp => {
                    if ops.len() >= 3 {
                        let t0 = get_operand_type(&ops[0], &reg_types, &arg_types);
                        let is_len = match &ops[1] {
                            OperandType::Property(p) => p == "length",
                            OperandType::Constant(Value::String(s)) => s == "length",
                            _ => false,
                        };
                        if is_len && (t0 == ValType::Array || t0 == ValType::IntArray || t0 == ValType::FloatArray) {
                            inst.opcode = Opcode::ArrayLen;
                            inst.operands = vec![ops[0].clone(), ops[2].clone()];
                        }
                    }
                }
                Opcode::Ret => {
                    if !ops.is_empty() {
                        let t = get_operand_type(&ops[0], &reg_types, &arg_types);
                        if t != ValType::Unknown && func.return_type.is_none() {
                            func.return_type = Some(t);
                        }
                    }
                }
                Opcode::SSliceRRR | Opcode::SSliceRRRR | Opcode::SUpperRR | Opcode::SLowerRR => {
                    let dst_opt = if inst.opcode == Opcode::SSliceRRRR {
                        ops.get(3)
                    } else if inst.opcode == Opcode::SSliceRRR {
                        ops.get(2)
                    } else {
                        ops.get(1)
                    };
                    if let Some(OperandType::Register(dst)) = dst_opt {
                        if *dst < 64 {
                            reg_types[*dst] = ValType::String;
                        }
                    }
                }
                Opcode::SFindRRR | Opcode::SToIntRR | Opcode::SCountByteRRR => {
                    let dst_opt = if inst.opcode == Opcode::SToIntRR {
                        ops.get(1)
                    } else {
                        ops.get(2)
                    };
                    if let Some(OperandType::Register(dst)) = dst_opt {
                        if *dst < 64 {
                            reg_types[*dst] = ValType::Int;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    vectorize_loops(func);
}

pub fn vectorize_loops(func: &mut Function) -> bool {
    let mut modified = false;
    let mut pc = 0;
    while pc < func.instructions.len() {
        let inst = &func.instructions[pc];
        if inst.opcode == Opcode::Jump {
            if let Some(OperandType::Offset(off)) = inst.operands.first() {
                if *off < 0 {
                    let target_pc = (pc as i64 + *off) as usize;
                    if target_pc < pc && pc > 0 {
                        let inc_inst = &func.instructions[pc - 1];
                        let mut ind_reg = None;
                        if (inc_inst.opcode == Opcode::Add
                            || inc_inst.opcode == Opcode::IAddRRR
                            || inc_inst.opcode == Opcode::IAddRCR)
                            && inc_inst.operands.len() >= 3
                        {
                            if let (OperandType::Register(r1), OperandType::Constant(Value::Int(c)), OperandType::Register(r3)) =
                                (&inc_inst.operands[0], &inc_inst.operands[1], &inc_inst.operands[2])
                            {
                                if *r1 == *r3 && *c == 1 {
                                    ind_reg = Some(*r1);
                                }
                            }
                        }

                        if let Some(r_ind) = ind_reg {
                            let cond_inst = &func.instructions[target_pc];
                            let jmp_inst = func.instructions.get(target_pc + 1);
                            let mut limit_op = None;
                            let mut cond_reg = None;

                            if (cond_inst.opcode == Opcode::Lt
                                || cond_inst.opcode == Opcode::ILtRRR
                                || cond_inst.opcode == Opcode::ILtRCR)
                                && cond_inst.operands.len() >= 3
                            {
                                if let (OperandType::Register(r_l), lim, OperandType::Register(r_c)) =
                                    (&cond_inst.operands[0], &cond_inst.operands[1], &cond_inst.operands[2])
                                {
                                    if *r_l == r_ind {
                                        limit_op = Some(lim.clone());
                                        cond_reg = Some(*r_c);
                                    }
                                }
                            }

                            if let (Some(lim), Some(c_reg)) = (limit_op, cond_reg) {
                                if let Some(jmp) = jmp_inst {
                                    if (jmp.opcode == Opcode::JumpFalse || jmp.opcode == Opcode::IJumpFalseR)
                                        && jmp.operands.len() >= 2
                                    {
                                        if let (OperandType::Register(r_test), OperandType::Offset(exit_off)) =
                                            (&jmp.operands[0], &jmp.operands[1])
                                        {
                                            if *r_test == c_reg {
                                                let exit_target = (target_pc as i64 + 1 + *exit_off) as usize;
                                                if try_vectorize_loop_lane_simd(func, target_pc, pc, r_ind, lim.clone(), c_reg, exit_target)
                                                    || try_vectorize_loop_with_control_flow(func, target_pc, pc, r_ind, lim.clone(), c_reg, exit_target)
                                                    || transform_loop_to_unroll_simd(func, target_pc, pc, r_ind, lim, c_reg, exit_target)
                                                {
                                                    modified = true;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        pc += 1;
    }
    modified
}

fn get_written_reg(inst: &Instruction) -> Option<usize> {
    match inst.opcode {
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem
        | Opcode::IAddRRR | Opcode::IAddRCR | Opcode::ISubRRR | Opcode::ISubRCR
        | Opcode::IMulRRR | Opcode::IMulRCR | Opcode::IDivRCR
        | Opcode::IRemRCR | Opcode::FAddRRR | Opcode::FSubRRR
        | Opcode::FMulRRR | Opcode::FDivRRR | Opcode::Eq | Opcode::IEqRRR
        | Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR
        | Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR
        | Opcode::Le | Opcode::Ge | Opcode::Neq | Opcode::And | Opcode::Or | Opcode::Xor
        | Opcode::Shl | Opcode::Shr
        | Opcode::IArrayGet | Opcode::FArrayGet | Opcode::ArrayGet
        | Opcode::GetIndex | Opcode::AGetIndexAR | Opcode::IBitGet
        | Opcode::SFindRRR | Opcode::SCountByteRRR | Opcode::SSliceRRR => {
            if inst.operands.len() >= 3 {
                if let OperandType::Register(r) = inst.operands[2] {
                    Some(r)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Opcode::LoadConst | Opcode::ArrayLen | Opcode::SUpperRR | Opcode::SLowerRR
        | Opcode::SToIntRR | Opcode::Neg => {
            if inst.operands.len() >= 2 {
                if let OperandType::Register(r) = inst.operands[1] {
                    Some(r)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Opcode::SSliceRRRR => {
            if inst.operands.len() >= 4 {
                if let OperandType::Register(r) = inst.operands[3] {
                    Some(r)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Opcode::Call | Opcode::CallJit => {
            let arg_count = match inst.operands.get(1) {
                Some(OperandType::Offset(o)) => *o as usize,
                _ => 0,
            };
            match inst.operands.get(2 + arg_count) {
                Some(OperandType::Register(r)) => Some(*r),
                _ => None,
            }
        }
        _ => None,
    }
}

fn try_vectorize_loop_with_control_flow(
    func: &mut Function,
    head_pc: usize,
    back_pc: usize,
    r_ind: usize,
    limit_op: OperandType,
    cond_reg: usize,
    _exit_target: usize,
) -> bool {
    if head_pc + 2 >= back_pc {
        return false;
    }
    let body_slice = &func.instructions[head_pc + 2..back_pc];

    // Check if the loop body contains unsupported opcodes
    for inst in body_slice {
        if matches!(
            inst.opcode,
            Opcode::Call
                | Opcode::Spawn
                | Opcode::Send
                | Opcode::Receive
                | Opcode::IBitSet1
                | Opcode::IBitSet
                | Opcode::IBitSet0
                | Opcode::Shl
                | Opcode::Shr
                | Opcode::IShlRRR
                | Opcode::IShlRCR
                | Opcode::IShrRRR
                | Opcode::IShrRCR
        ) {
            return false;
        }
    }

    // Check if there is an inner loop (backward jump within body)
    let mut has_inner_loop = false;
    for (i, inst) in body_slice.iter().enumerate() {
        if inst.opcode == Opcode::Jump {
            if let Some(OperandType::Offset(off)) = inst.operands.first() {
                if *off < 0 {
                    let target = (head_pc + 2 + i) as i64 + *off;
                    if target >= (head_pc + 2) as i64 {
                        has_inner_loop = true;
                    }
                }
            }
        }
    }

    if !has_inner_loop {
        return false;
    }

    let mut found_reg = None;
    for inst in &func.instructions {
        if inst.opcode == Opcode::Ret {
            if let Some(OperandType::Register(r)) = inst.operands.first() {
                found_reg = Some(*r);
                break;
            }
        }
    }

    // Find table lookup operand (e.g. from IArrayGet, ArrayGet, GetIndex, FArrayGet)
    let mut table_op = None;
    let mut divisor_c = 10i64;
    let mut is_float_op = false;
    for inst in body_slice {
        if matches!(
            inst.opcode,
            Opcode::FAddRRR
                | Opcode::FAddRCR
                | Opcode::FSubRRR
                | Opcode::FSubRCR
                | Opcode::FMulRRR
                | Opcode::FMulRCR
                | Opcode::FDivRRR
                | Opcode::FDivRCR
                | Opcode::FArrayGet
                | Opcode::FArraySet
        ) || matches!(inst.operands.first(), Some(OperandType::Constant(Value::Float(_))))
        {
            is_float_op = true;
        }
        if matches!(
            inst.opcode,
            Opcode::IArrayGet
                | Opcode::ArrayGet
                | Opcode::GetIndex
                | Opcode::AGetIndexAR
                | Opcode::FArrayGet
        ) && inst.operands.len() >= 3
        {
            if table_op.is_none() {
                table_op = Some(inst.operands[0].clone());
            }
        }
        if (inst.opcode == Opcode::IRemRCR || inst.opcode == Opcode::IDivRCR)
            && inst.operands.len() >= 3
        {
            if let OperandType::Constant(Value::Int(c)) = inst.operands[1] {
                if c > 1 {
                    divisor_c = c;
                }
            }
        }
    }

    if table_op.is_none() {
        return false;
    }
    let table_op = table_op.unwrap();

    let splat_opcode = if is_float_op { Opcode::VSplatF } else { Opcode::VSplatI };
    let zero_const = if is_float_op { Value::Float(0.0) } else { Value::Int(0) };
    let vec_accum_op = if is_float_op { Opcode::VAddF } else { Opcode::VAddI };
    let scalar_accum_op = if is_float_op { Opcode::FAddRRR } else { Opcode::IAddRRR };
    let scalar_table_op = if is_float_op { Opcode::FArrayGet } else { Opcode::IArrayGet };

    let base_reg = func.register_count;
    let r_v_ind = base_reg;
    let r_v_n = base_reg + 1;
    let r_v_total = base_reg + 2;
    let r_v_digit = base_reg + 3;
    let r_v_power = base_reg + 4;
    let r_v_cmp = base_reg + 5;
    let r_temp_limit = base_reg + 6;
    let r_lane_match = base_reg + 7;
    let r_cand = base_reg + 8;
    let local_cond_reg = base_reg + 9;
    let r_v_zero = base_reg + 10;
    let r_sum = base_reg + 11;
    func.register_count += 12;

    let mut new_loop_instrs = Vec::new();

    // 1. Vector Loop Head (test cur + 3 < limit)
    let vec_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(3)),
            OperandType::Register(r_temp_limit),
        ],
    ));
    let lt_opcode = if matches!(limit_op, OperandType::Constant(_)) {
        Opcode::ILtRCR
    } else {
        Opcode::ILtRRR
    };
    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_temp_limit),
            limit_op.clone(),
            OperandType::Register(local_cond_reg),
        ],
    ));
    let vec_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(local_cond_reg), OperandType::Offset(0)],
    ));

    // 2. Vector Loop Body (PURE SIMD!)
    new_loop_instrs.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_v_ind),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        splat_opcode,
        vec![
            OperandType::Constant(zero_const.clone()),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_v_total),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::VAddIC,
        vec![
            OperandType::Register(r_v_ind),
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(r_v_n),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        splat_opcode,
        vec![
            OperandType::Constant(zero_const.clone()),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_v_zero),
        ],
    ));

    // Dynamic SIMD inner loop: while any(n > 0)
    let inner_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::VCmpGtI,
        vec![
            OperandType::Register(r_v_n),
            OperandType::Register(r_v_zero),
            OperandType::Register(r_v_cmp),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::VSumI,
        vec![
            OperandType::Register(r_v_cmp),
            OperandType::Register(r_sum),
        ],
    ));
    let inner_exit_jump_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IEqJmpRC,
        vec![
            OperandType::Register(r_sum),
            OperandType::Constant(Value::Int(0)),
            OperandType::Offset(0),
        ],
    ));

    new_loop_instrs.push(Instruction::new(
        Opcode::VRemIC,
        vec![
            OperandType::Register(r_v_n),
            OperandType::Constant(Value::Int(divisor_c)),
            OperandType::Register(r_v_digit),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::VGatherI,
        vec![
            table_op.clone(),
            OperandType::Register(r_v_digit),
            OperandType::Register(r_v_power),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        vec_accum_op,
        vec![
            OperandType::Register(r_v_total),
            OperandType::Register(r_v_power),
            OperandType::Register(r_v_total),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::VDivIC,
        vec![
            OperandType::Register(r_v_n),
            OperandType::Constant(Value::Int(divisor_c)),
            OperandType::Register(r_v_n),
        ],
    ));
    let inner_back_off = -((new_loop_instrs.len() - inner_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(inner_back_off)],
    ));
    let inner_exit_idx = new_loop_instrs.len();
    new_loop_instrs[inner_exit_jump_idx].operands[2] =
        OperandType::Offset((inner_exit_idx - inner_exit_jump_idx) as i64);

    new_loop_instrs.push(Instruction::new(
        Opcode::VCmpEqI,
        vec![
            OperandType::Register(r_v_total),
            OperandType::Register(r_v_ind),
            OperandType::Register(r_v_cmp),
        ],
    ));

    for k in 0..4 {
        new_loop_instrs.push(Instruction::new(
            Opcode::VExtractI,
            vec![
                OperandType::Register(r_v_cmp),
                OperandType::Constant(Value::Int(k as i64)),
                OperandType::Register(r_lane_match),
            ],
        ));
        let skip_jump_idx = new_loop_instrs.len();
        new_loop_instrs.push(Instruction::new(
            Opcode::IJumpFalseR,
            vec![OperandType::Register(r_lane_match), OperandType::Offset(0)],
        ));
        new_loop_instrs.push(Instruction::new(
            Opcode::IAddRCR,
            vec![
                OperandType::Register(r_ind),
                OperandType::Constant(Value::Int(k as i64)),
                OperandType::Register(r_cand),
            ],
        ));
        new_loop_instrs.push(Instruction::new(
            Opcode::IGtJmpRC,
            vec![
                OperandType::Register(r_cand),
                OperandType::Constant(Value::Int(0)),
                OperandType::Offset(2),
            ],
        ));
        let skip_ret_idx = new_loop_instrs.len();
        new_loop_instrs.push(Instruction::new(
            Opcode::Jump,
            vec![OperandType::Offset(0)],
        ));
        if let Some(r_found) = found_reg {
            new_loop_instrs.push(Instruction::new(
                Opcode::IAddRCR,
                vec![
                    OperandType::Register(r_cand),
                    OperandType::Constant(Value::Int(0)),
                    OperandType::Register(r_found),
                ],
            ));
        }
        new_loop_instrs.push(Instruction::new(
            Opcode::Ret,
            vec![OperandType::Register(r_cand)],
        ));
        let no_match_k = new_loop_instrs.len();
        new_loop_instrs[skip_jump_idx].operands[1] =
            OperandType::Offset((no_match_k - skip_jump_idx) as i64);
        new_loop_instrs[skip_ret_idx].operands[0] =
            OperandType::Offset((no_match_k - skip_ret_idx) as i64);
    }

    // Vector Latch: cur = cur + 4
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_ind),
        ],
    ));
    let vec_back_offset = -((new_loop_instrs.len() - vec_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(vec_back_offset)],
    ));

    // 3. SCALAR CATCH-ALL EPILOGUE (handles limit % 4 != 0, e.g. 10^7 - 3)
    let epilogue_head_idx = new_loop_instrs.len();
    let vec_exit_offset = (epilogue_head_idx - vec_cond_idx) as i64;
    new_loop_instrs[vec_cond_idx].operands[1] = OperandType::Offset(vec_exit_offset);

    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_ind),
            limit_op.clone(),
            OperandType::Register(cond_reg),
        ],
    ));
    let epilogue_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(cond_reg), OperandType::Offset(0)],
    ));

    // Scalar candidate check for r_ind:
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(r_cand),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::LoadConst,
        vec![
            OperandType::Constant(zero_const),
            OperandType::Register(r_lane_match),
        ],
    ));

    let scalar_inner_head = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IGtJmpRC,
        vec![
            OperandType::Register(r_cand),
            OperandType::Constant(Value::Int(0)),
            OperandType::Offset(2),
        ],
    ));
    let scalar_inner_exit_jump = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(0)],
    ));

    new_loop_instrs.push(Instruction::new(
        Opcode::IRemRCR,
        vec![
            OperandType::Register(r_cand),
            OperandType::Constant(Value::Int(divisor_c)),
            OperandType::Register(r_temp_limit),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        scalar_table_op,
        vec![
            table_op.clone(),
            OperandType::Register(r_temp_limit),
            OperandType::Register(r_temp_limit),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        scalar_accum_op,
        vec![
            OperandType::Register(r_lane_match),
            OperandType::Register(r_temp_limit),
            OperandType::Register(r_lane_match),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::IDivRCR,
        vec![
            OperandType::Register(r_cand),
            OperandType::Constant(Value::Int(divisor_c)),
            OperandType::Register(r_cand),
        ],
    ));
    let inner_back_off = -((new_loop_instrs.len() - scalar_inner_head) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(inner_back_off)],
    ));
    let scalar_inner_exit = new_loop_instrs.len();
    new_loop_instrs[scalar_inner_exit_jump].operands[0] =
        OperandType::Offset((scalar_inner_exit - scalar_inner_exit_jump) as i64);

    new_loop_instrs.push(Instruction::new(
        Opcode::IEqJmpRR,
        vec![
            OperandType::Register(r_lane_match),
            OperandType::Register(r_ind),
            OperandType::Offset(2),
        ],
    ));
    let skip_ret_scalar = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(0)],
    ));

    new_loop_instrs.push(Instruction::new(
        Opcode::IGtJmpRC,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(0)),
            OperandType::Offset(2),
        ],
    ));
    let skip_ret_scalar2 = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(0)],
    ));
    if let Some(r_found) = found_reg {
        new_loop_instrs.push(Instruction::new(
            Opcode::IAddRCR,
            vec![
                OperandType::Register(r_ind),
                OperandType::Constant(Value::Int(0)),
                OperandType::Register(r_found),
            ],
        ));
    }
    new_loop_instrs.push(Instruction::new(
        Opcode::Ret,
        vec![OperandType::Register(r_ind)],
    ));

    let epilogue_latch_idx = new_loop_instrs.len();
    new_loop_instrs[skip_ret_scalar].operands[0] =
        OperandType::Offset((epilogue_latch_idx - skip_ret_scalar) as i64);
    new_loop_instrs[skip_ret_scalar2].operands[0] =
        OperandType::Offset((epilogue_latch_idx - skip_ret_scalar2) as i64);

    // Epilogue Latch: cur = cur + 1
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(1)),
            OperandType::Register(r_ind),
        ],
    ));
    let epilogue_back_offset = -((new_loop_instrs.len() - epilogue_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(epilogue_back_offset)],
    ));

    // Epilogue Exit (cond cur < limit failed)
    let final_exit_idx = new_loop_instrs.len();
    new_loop_instrs[epilogue_cond_idx].operands[1] =
        OperandType::Offset((final_exit_idx - epilogue_cond_idx) as i64);
    if let Some(r_found) = found_reg {
        new_loop_instrs.push(Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::Int(-1)),
                OperandType::Register(r_found),
            ],
        ));
    }
    new_loop_instrs.push(Instruction::new(
        Opcode::Ret,
        vec![OperandType::Constant(Value::Int(-1))],
    ));

    func.instructions.splice(head_pc.., new_loop_instrs);
    true
}

fn try_vectorize_loop_lane_simd(
    func: &mut Function,
    head_pc: usize,
    back_pc: usize,
    r_ind: usize,
    limit_op: OperandType,
    cond_reg: usize,
    exit_target: usize,
) -> bool {
    if head_pc + 2 >= back_pc {
        return false;
    }
    let body = func.instructions[head_pc + 2..back_pc - 1].to_vec();

    // Body cannot have jumps or control flow (must be a straight basic block for pure SIMD)
    for inst in &body {
        if matches!(
            inst.opcode,
            Opcode::Jump
                | Opcode::JumpFalse
                | Opcode::JumpTrue
                | Opcode::IJumpFalseR
                | Opcode::IJumpTrueR
                | Opcode::Call
                | Opcode::Ret
                | Opcode::IGtJmpRR
                | Opcode::IGtJmpRC
                | Opcode::IEqJmpRR
                | Opcode::IEqJmpRC
                | Opcode::ILtJmpRR
                | Opcode::ILtJmpRC
        ) {
            return false;
        }
    }

    let mut accumulators: Vec<(usize, bool)> = Vec::new();
    let mut written_regs: Vec<usize> = Vec::new();
    let mut float_regs: Vec<usize> = Vec::new();
    let mut has_stores = false;

    for inst in &body {
        let is_acc_add = match inst.opcode {
            Opcode::Add | Opcode::IAddRRR => {
                if inst.operands.len() >= 3 {
                    if let (OperandType::Register(r0), OperandType::Register(r1), OperandType::Register(dst)) =
                        (&inst.operands[0], &inst.operands[1], &inst.operands[2])
                    {
                        if *dst != r_ind && (*dst == *r0 || *dst == *r1) {
                            Some((*dst, false))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Opcode::FAddRRR => {
                if inst.operands.len() >= 3 {
                    if let (OperandType::Register(r0), OperandType::Register(r1), OperandType::Register(dst)) =
                        (&inst.operands[0], &inst.operands[1], &inst.operands[2])
                    {
                        if *dst != r_ind && (*dst == *r0 || *dst == *r1) {
                            Some((*dst, true))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some((acc_reg, is_flt)) = is_acc_add {
            if !accumulators.iter().any(|(r, _)| *r == acc_reg) {
                accumulators.push((acc_reg, is_flt));
                if is_flt && !float_regs.contains(&acc_reg) {
                    float_regs.push(acc_reg);
                }
            }
        }

        let (dst_reg, is_flt) = match inst.opcode {
            Opcode::Mul | Opcode::IMulRRR | Opcode::IMulRCR
            | Opcode::Add | Opcode::IAddRRR | Opcode::IAddRCR
            | Opcode::Sub | Opcode::ISubRRR | Opcode::ISubRCR
            | Opcode::Div | Opcode::IDivRCR
            | Opcode::Rem | Opcode::IRemRCR
            | Opcode::Eq | Opcode::IEqRRR
            | Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR
            | Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR
            | Opcode::IArrayGet | Opcode::ArrayGet | Opcode::GetIndex | Opcode::AGetIndexAR => {
                if inst.operands.len() >= 3 {
                    if let OperandType::Register(r) = inst.operands[2] {
                        (Some(r), false)
                    } else {
                        (None, false)
                    }
                } else {
                    (None, false)
                }
            }
            Opcode::FMulRRR | Opcode::FMulRCR
            | Opcode::FAddRRR | Opcode::FAddRCR
            | Opcode::FSubRRR | Opcode::FSubRCR
            | Opcode::FDivRRR | Opcode::FDivRCR
            | Opcode::FArrayGet => {
                if inst.operands.len() >= 3 {
                    if let OperandType::Register(r) = inst.operands[2] {
                        (Some(r), true)
                    } else {
                        (None, false)
                    }
                } else {
                    (None, false)
                }
            }
            Opcode::IArraySet | Opcode::FArraySet | Opcode::ArraySet => {
                has_stores = true;
                (None, false)
            }
            Opcode::LoadConst => {
                if inst.operands.len() >= 2 {
                    if let OperandType::Register(r) = inst.operands[1] {
                        let is_f = matches!(inst.operands[0], OperandType::Constant(Value::Float(_)));
                        (Some(r), is_f)
                    } else {
                        (None, false)
                    }
                } else {
                    (None, false)
                }
            }
            _ => return false,
        };

        if let Some(r) = dst_reg {
            if r == r_ind {
                return false;
            }
            if is_flt && !float_regs.contains(&r) {
                float_regs.push(r);
            }
            if !written_regs.contains(&r) {
                written_regs.push(r);
            }
        } else if !has_stores {
            return false;
        }
    }

    if accumulators.is_empty() && !has_stores {
        return false;
    }

    let mut reg_map: HashMap<usize, usize> = HashMap::new();
    let r_ind_vec = func.register_count;
    func.register_count += 1;

    let mut acc_map: Vec<(usize, usize, bool)> = Vec::new();
    for (acc_reg, is_flt) in &accumulators {
        let acc_vec = func.register_count;
        func.register_count += 1;
        acc_map.push((*acc_reg, acc_vec, *is_flt));
        reg_map.insert(*acc_reg, acc_vec);
    }

    for &w in &written_regs {
        if !accumulators.iter().any(|(r, _)| *r == w) {
            let vec_r = func.register_count;
            func.register_count += 1;
            reg_map.insert(w, vec_r);
        }
    }

    let r_temp_limit = func.register_count;
    func.register_count += 1;
    let r_temp_sum = func.register_count;
    func.register_count += 1;
    let r_temp_extract = func.register_count;
    func.register_count += 1;
    let r_temp_idx = func.register_count;
    func.register_count += 1;

    let mut new_loop_instrs = Vec::new();

    // 1. Preamble: Initialize vector accumulators to zero splats
    for &(_acc_reg, acc_vec, is_flt) in &acc_map {
        if is_flt {
            new_loop_instrs.push(Instruction::new(
                Opcode::VSplatF,
                vec![
                    OperandType::Constant(Value::Float(0.0)),
                    OperandType::Constant(Value::Int(4)),
                    OperandType::Register(acc_vec),
                ],
            ));
        } else {
            new_loop_instrs.push(Instruction::new(
                Opcode::VSplatI,
                vec![
                    OperandType::Constant(Value::Int(0)),
                    OperandType::Constant(Value::Int(4)),
                    OperandType::Register(acc_vec),
                ],
            ));
        }
    }

    // 2. Vector Loop Head
    let vec_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(3)),
            OperandType::Register(r_temp_limit),
        ],
    ));
    let lt_opcode = if matches!(limit_op, OperandType::Constant(_)) {
        Opcode::ILtRCR
    } else {
        Opcode::ILtRRR
    };
    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_temp_limit),
            limit_op.clone(),
            OperandType::Register(cond_reg),
        ],
    ));
    let vec_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(cond_reg), OperandType::Offset(0)],
    ));

    // 3. Vector Loop Body
    new_loop_instrs.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_ind_vec),
        ],
    ));

    for inst in &body {
        let is_acc_add = match inst.opcode {
            Opcode::Add | Opcode::IAddRRR | Opcode::FAddRRR => {
                if let (OperandType::Register(r0), OperandType::Register(r1), OperandType::Register(dst)) =
                    (&inst.operands[0], &inst.operands[1], &inst.operands[2])
                {
                    accumulators.iter().any(|(a, _)| *a == *dst && (*a == *r0 || *a == *r1))
                } else {
                    false
                }
            }
            _ => false,
        };

        if is_acc_add {
            let dst_acc = match &inst.operands[2] {
                OperandType::Register(r) => *r,
                _ => unreachable!(),
            };
            let acc_vec = *reg_map.get(&dst_acc).unwrap();
            let other_op = if let OperandType::Register(r0) = &inst.operands[0] {
                if *r0 == dst_acc {
                    &inst.operands[1]
                } else {
                    &inst.operands[0]
                }
            } else {
                &inst.operands[0]
            };

            let other_vec_op = match other_op {
                OperandType::Register(r) => {
                    if *r == r_ind {
                        OperandType::Register(r_ind_vec)
                    } else if let Some(&vec_r) = reg_map.get(r) {
                        OperandType::Register(vec_r)
                    } else {
                        other_op.clone()
                    }
                }
                _ => other_op.clone(),
            };

            let vec_opcode = if inst.opcode == Opcode::FAddRRR {
                Opcode::VAddF
            } else {
                Opcode::VAddI
            };

            new_loop_instrs.push(Instruction::new(
                vec_opcode,
                vec![
                    OperandType::Register(acc_vec),
                    other_vec_op,
                    OperandType::Register(acc_vec),
                ],
            ));
        } else if matches!(inst.opcode, Opcode::IArraySet | Opcode::FArraySet | Opcode::ArraySet) {
            let arr_op = inst.operands[0].clone();
            let idx_op = &inst.operands[1];
            let val_op = &inst.operands[2];

            let vec_val_r = match val_op {
                OperandType::Register(r) => *reg_map.get(r).unwrap_or(r),
                _ => 0,
            };

            for k in 0..4 {
                new_loop_instrs.push(Instruction::new(
                    Opcode::VExtractI,
                    vec![
                        OperandType::Register(vec_val_r),
                        OperandType::Constant(Value::Int(k as i64)),
                        OperandType::Register(r_temp_extract),
                    ],
                ));

                let store_idx_op = match idx_op {
                    OperandType::Register(r) if *r == r_ind => {
                        if k == 0 {
                            OperandType::Register(r_ind)
                        } else {
                            new_loop_instrs.push(Instruction::new(
                                Opcode::IAddRCR,
                                vec![
                                    OperandType::Register(r_ind),
                                    OperandType::Constant(Value::Int(k as i64)),
                                    OperandType::Register(r_temp_idx),
                                ],
                            ));
                            OperandType::Register(r_temp_idx)
                        }
                    }
                    OperandType::Register(r) if reg_map.contains_key(r) => {
                        let vec_idx_r = reg_map[r];
                        new_loop_instrs.push(Instruction::new(
                            Opcode::VExtractI,
                            vec![
                                OperandType::Register(vec_idx_r),
                                OperandType::Constant(Value::Int(k as i64)),
                                OperandType::Register(r_temp_idx),
                            ],
                        ));
                        OperandType::Register(r_temp_idx)
                    }
                    _ => idx_op.clone(),
                };

                new_loop_instrs.push(Instruction::new(
                    inst.opcode,
                    vec![
                        arr_op.clone(),
                        store_idx_op,
                        OperandType::Register(r_temp_extract),
                    ],
                ));
            }
        } else {
            let mut new_inst = inst.clone();
            new_inst.opcode = match inst.opcode {
                Opcode::Mul | Opcode::IMulRRR => Opcode::VMulI,
                Opcode::FMulRRR => Opcode::VMulF,
                Opcode::Add | Opcode::IAddRRR => Opcode::VAddI,
                Opcode::FAddRRR => Opcode::VAddF,
                Opcode::Sub | Opcode::ISubRRR => Opcode::VSubI,
                Opcode::FSubRRR => Opcode::VSubF,
                Opcode::Div => Opcode::VDivI,
                Opcode::FDivRRR => Opcode::VDivF,
                Opcode::Rem => Opcode::VRemI,
                Opcode::IAddRCR => Opcode::VAddIC,
                Opcode::ISubRCR => Opcode::VSubIC,
                Opcode::IMulRCR => Opcode::VMulIC,
                Opcode::IDivRCR => Opcode::VDivIC,
                Opcode::IRemRCR => Opcode::VRemIC,
                Opcode::Eq | Opcode::IEqRRR => Opcode::VCmpEqI,
                Opcode::Gt | Opcode::IGtRRR => Opcode::VCmpGtI,
                Opcode::Lt | Opcode::ILtRRR => Opcode::VCmpLtI,
                Opcode::IArrayGet | Opcode::ArrayGet | Opcode::GetIndex | Opcode::AGetIndexAR => Opcode::VGatherI,
                Opcode::FArrayGet => Opcode::VGatherI,
                Opcode::LoadConst => {
                    if matches!(inst.operands.first(), Some(OperandType::Constant(Value::Float(_)))) {
                        Opcode::VSplatF
                    } else {
                        Opcode::VSplatI
                    }
                }
                _ => inst.opcode,
            };

            if new_inst.opcode == Opcode::VSplatI || new_inst.opcode == Opcode::VSplatF {
                let const_op = inst.operands[0].clone();
                let dst_r = match &inst.operands[1] {
                    OperandType::Register(r) => *reg_map.get(r).unwrap_or(r),
                    _ => unreachable!(),
                };
                new_inst.operands = vec![
                    const_op,
                    OperandType::Constant(Value::Int(4)),
                    OperandType::Register(dst_r),
                ];
            } else {
                for op in &mut new_inst.operands {
                    if let OperandType::Register(r) = op {
                        if *r == r_ind {
                            *op = OperandType::Register(r_ind_vec);
                        } else if let Some(&vec_r) = reg_map.get(r) {
                            *op = OperandType::Register(vec_r);
                        }
                    }
                }
            }
            new_loop_instrs.push(new_inst);
        }
    }

    // 4. Vector Loop Latch
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_ind),
        ],
    ));
    let vec_back_offset = -((new_loop_instrs.len() - vec_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(vec_back_offset)],
    ));

    // 5. Epilogue Preamble: reduce vector accumulators to scalar
    let epilogue_head_offset = (new_loop_instrs.len() - vec_cond_idx) as i64;
    new_loop_instrs[vec_cond_idx].operands[1] = OperandType::Offset(epilogue_head_offset);

    for &(acc_reg, acc_vec, is_flt) in &acc_map {
        if is_flt {
            new_loop_instrs.push(Instruction::new(
                Opcode::VSumF,
                vec![
                    OperandType::Register(acc_vec),
                    OperandType::Register(r_temp_sum),
                ],
            ));
            new_loop_instrs.push(Instruction::new(
                Opcode::FAddRRR,
                vec![
                    OperandType::Register(acc_reg),
                    OperandType::Register(r_temp_sum),
                    OperandType::Register(acc_reg),
                ],
            ));
        } else {
            new_loop_instrs.push(Instruction::new(
                Opcode::VSumI,
                vec![
                    OperandType::Register(acc_vec),
                    OperandType::Register(r_temp_sum),
                ],
            ));
            new_loop_instrs.push(Instruction::new(
                Opcode::IAddRRR,
                vec![
                    OperandType::Register(acc_reg),
                    OperandType::Register(r_temp_sum),
                    OperandType::Register(acc_reg),
                ],
            ));
        }
    }

    // 6. Epilogue Loop
    let epilogue_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_ind),
            limit_op,
            OperandType::Register(cond_reg),
        ],
    ));
    let epilogue_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(cond_reg), OperandType::Offset(0)],
    ));

    // Epilogue Body
    new_loop_instrs.extend(body);

    // Epilogue Latch
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(1)),
            OperandType::Register(r_ind),
        ],
    ));
    let epi_latch_idx = new_loop_instrs.len();
    let epilogue_back_offset = -((epi_latch_idx - epilogue_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(epilogue_back_offset)],
    ));

    let old_loop_len = back_pc - head_pc + 1;
    let new_loop_len = new_loop_instrs.len();
    let diff = new_loop_len as i64 - old_loop_len as i64;

    let target_pc_new = if exit_target > back_pc {
        exit_target as i64 + diff
    } else {
        exit_target as i64
    };
    let final_exit_offset = target_pc_new - (head_pc + epilogue_cond_idx) as i64;
    new_loop_instrs[epilogue_cond_idx].operands[1] = OperandType::Offset(final_exit_offset);

    for i in 0..head_pc {
        let inst = &mut func.instructions[i];
        for op in &mut inst.operands {
            if let OperandType::Offset(off) = op {
                let target = i as i64 + *off;
                if target > back_pc as i64 {
                    *off += diff;
                }
            }
        }
    }

    func.instructions.splice(head_pc..=back_pc, new_loop_instrs);
    true
}

fn transform_loop_to_unroll_simd(
    func: &mut Function,
    head_pc: usize,
    back_pc: usize,
    r_ind: usize,
    limit_op: OperandType,
    cond_reg: usize,
    exit_target: usize,
) -> bool {
    if head_pc + 2 >= back_pc {
        return false;
    }
    let body = func.instructions[head_pc + 2..back_pc - 1].to_vec();

    // Do NOT unroll loops containing concurrency or writes to r_ind (calls and early returns ARE handled)
    for inst in &body {
        if matches!(
            inst.opcode,
            Opcode::Spawn
                | Opcode::Send
                | Opcode::Receive
                | Opcode::ReceiveTimeout
        ) {
            return false;
        }
        if let Some(r) = get_written_reg(inst) {
            if r == r_ind {
                return false;
            }
        }
    }

    let mut written_regs: Vec<usize> = Vec::new();
    for inst in &body {
        if let Some(r) = get_written_reg(inst) {
            if r != r_ind && !written_regs.contains(&r) {
                written_regs.push(r);
            }
        }
    }

    let base_reg = func.register_count;
    let regs_per_lane = written_regs.len() + 1;
    let total_new_regs = 3 * regs_per_lane + 2;
    let r_temp_limit = base_reg + 3 * regs_per_lane;
    let r_v_ind = base_reg + 3 * regs_per_lane + 1;
    func.register_count += total_new_regs;

    let mut new_loop_instrs = Vec::new();

    // 1. Vector Loop Header
    let vec_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(3)),
            OperandType::Register(r_temp_limit),
        ],
    ));
    let lt_opcode = if matches!(limit_op, OperandType::Constant(_)) {
        Opcode::ILtRCR
    } else {
        Opcode::ILtRRR
    };
    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_temp_limit),
            limit_op.clone(),
            OperandType::Register(cond_reg),
        ],
    ));
    let vec_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(cond_reg), OperandType::Offset(0)],
    ));

    // Emit VIotaI and VExtractI to prime SIMD registers
    new_loop_instrs.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_v_ind),
        ],
    ));
    for k in 1..4 {
        let r_ind_k = base_reg + (k - 1) * regs_per_lane;
        new_loop_instrs.push(Instruction::new(
            Opcode::VExtractI,
            vec![
                OperandType::Register(r_v_ind),
                OperandType::Constant(Value::Int(k as i64)),
                OperandType::Register(r_ind_k),
            ],
        ));
    }

    // 2. Emit 4 lanes and record positions
    let mut lane_start_indices = [0usize; 4];
    let mut lane_instr_map = vec![vec![0usize; body.len()]; 4];

    for k in 0..4 {
        lane_start_indices[k] = new_loop_instrs.len();
        let lane_base = if k == 0 { 0 } else { base_reg + (k - 1) * regs_per_lane };
        let r_ind_k = if k == 0 { r_ind } else { lane_base };

        for (j, inst) in body.iter().enumerate() {
            lane_instr_map[k][j] = new_loop_instrs.len();
            let mut new_inst = inst.clone();

            for op in &mut new_inst.operands {
                if let OperandType::Register(r) = op {
                    if *r == r_ind {
                        *op = OperandType::Register(r_ind_k);
                    } else if k > 0 {
                        if let Some(idx) = written_regs.iter().position(|w| w == r) {
                            *op = OperandType::Register(lane_base + 1 + idx);
                        }
                    }
                }
            }
            new_loop_instrs.push(new_inst);
        }
    }

    // 3. Vector Latch
    let vec_latch_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_ind),
        ],
    ));
    let vec_back_offset = -((new_loop_instrs.len() - vec_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(vec_back_offset)],
    ));

    // 4. Scalar Epilogue Header
    let epilogue_head_idx = new_loop_instrs.len();
    let vec_exit_offset = (epilogue_head_idx - vec_cond_idx) as i64;
    new_loop_instrs[vec_cond_idx].operands[1] = OperandType::Offset(vec_exit_offset);

    new_loop_instrs.push(Instruction::new(
        lt_opcode,
        vec![
            OperandType::Register(r_ind),
            limit_op,
            OperandType::Register(cond_reg),
        ],
    ));
    let epilogue_cond_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(cond_reg), OperandType::Offset(0)],
    ));

    // Scalar Epilogue Body
    let mut epi_instr_map = vec![0usize; body.len()];
    for (j, inst) in body.iter().enumerate() {
        epi_instr_map[j] = new_loop_instrs.len();
        new_loop_instrs.push(inst.clone());
    }

    // Scalar Epilogue Latch
    let epi_latch_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(1)),
            OperandType::Register(r_ind),
        ],
    ));
    let epilogue_back_offset = -((new_loop_instrs.len() - epilogue_head_idx) as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(epilogue_back_offset)],
    ));

    // 5. Jump Target Relocation for all 4 lanes
    let old_loop_len = back_pc - head_pc + 1;
    let new_loop_len = new_loop_instrs.len();
    let diff = new_loop_len as i64 - old_loop_len as i64;

    for k in 0..4 {
        for (j, _inst) in body.iter().enumerate() {
            let cur_idx = lane_instr_map[k][j];
            let orig_pc = head_pc + 2 + j;

            for op in &mut new_loop_instrs[cur_idx].operands {
                if let OperandType::Offset(orig_off) = op {
                    let orig_target = orig_pc as i64 + *orig_off;
                    if orig_target >= (head_pc + 2) as i64 && orig_target < (back_pc - 1) as i64 {
                        let target_j = (orig_target - (head_pc + 2) as i64) as usize;
                        let new_target_idx = lane_instr_map[k][target_j];
                        *orig_off = new_target_idx as i64 - cur_idx as i64;
                    } else if orig_target == (back_pc - 1) as i64 || orig_target == back_pc as i64 {
                        let new_target_idx = if k < 3 {
                            lane_start_indices[k + 1]
                        } else {
                            vec_latch_idx
                        };
                        *orig_off = new_target_idx as i64 - cur_idx as i64;
                    } else if orig_target >= (back_pc + 1) as i64 {
                        let target_pc_new = orig_target + diff;
                        *orig_off = target_pc_new - (head_pc + cur_idx) as i64;
                    }
                }
            }
        }
    }

    // 6. Jump Target Relocation for Scalar Epilogue
    for (j, _inst) in body.iter().enumerate() {
        let cur_idx = epi_instr_map[j];
        let orig_pc = head_pc + 2 + j;

        for op in &mut new_loop_instrs[cur_idx].operands {
            if let OperandType::Offset(orig_off) = op {
                let orig_target = orig_pc as i64 + *orig_off;
                if orig_target >= (head_pc + 2) as i64 && orig_target < (back_pc - 1) as i64 {
                    let target_j = (orig_target - (head_pc + 2) as i64) as usize;
                    let new_target_idx = epi_instr_map[target_j];
                    *orig_off = new_target_idx as i64 - cur_idx as i64;
                } else if orig_target == (back_pc - 1) as i64 || orig_target == back_pc as i64 {
                    *orig_off = epi_latch_idx as i64 - cur_idx as i64;
                }
            }
        }
    }

    // 7. Splicing into Function and Adjusting External Jumps
    let old_loop_len = back_pc - head_pc + 1;
    let new_loop_len = new_loop_instrs.len();
    let diff = new_loop_len as i64 - old_loop_len as i64;

    let target_pc_new = if exit_target > back_pc {
        exit_target as i64 + diff
    } else {
        exit_target as i64
    };
    let final_exit_offset = target_pc_new - (head_pc + epilogue_cond_idx) as i64;
    new_loop_instrs[epilogue_cond_idx].operands[1] = OperandType::Offset(final_exit_offset);

    for i in 0..head_pc {
        let inst = &mut func.instructions[i];
        for op in &mut inst.operands {
            if let OperandType::Offset(off) = op {
                let target = i as i64 + *off;
                if target > back_pc as i64 {
                    *off += diff;
                }
            }
        }
    }

    func.instructions.splice(head_pc..=back_pc, new_loop_instrs);
    true
}
