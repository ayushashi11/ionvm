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
                Opcode::Add | Opcode::IAddRRR | Opcode::FAddRRR => {
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

                            if (cond_inst.opcode == Opcode::Lt || cond_inst.opcode == Opcode::ILtRRR)
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
                                                if exit_target == pc + 1 {
                                                    if transform_loop_to_simd(func, target_pc, pc, r_ind, lim, c_reg) {
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
        }
        pc += 1;
    }
    modified
}

fn transform_loop_to_simd(
    func: &mut Function,
    head_pc: usize,
    back_pc: usize,
    r_ind: usize,
    limit_op: OperandType,
    cond_reg: usize,
) -> bool {
    if head_pc + 2 >= back_pc {
        return false;
    }
    let body = func.instructions[head_pc + 2..back_pc - 1].to_vec();

    let mut written_regs = Vec::new();
    for inst in &body {
        let dst_reg = match inst.opcode {
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem
            | Opcode::IAddRRR | Opcode::IAddRCR | Opcode::ISubRRR | Opcode::IMulRRR
            | Opcode::IDivRCR | Opcode::IRemRCR | Opcode::IEqRRR | Opcode::ILtRRR
            | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::FAddRRR | Opcode::FSubRRR
            | Opcode::FMulRRR | Opcode::FDivRRR | Opcode::Eq | Opcode::Lt | Opcode::Gt
            | Opcode::Le | Opcode::Ge | Opcode::Neq | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::GetIndex | Opcode::AGetIndexAR | Opcode::ArrayGet
            | Opcode::IArrayGet | Opcode::FArrayGet | Opcode::IBitGet => {
                if inst.operands.len() >= 3 {
                    if let OperandType::Register(r) = inst.operands[2] {
                        Some(r)
                    } else { None }
                } else { None }
            }
            Opcode::LoadConst | Opcode::ArrayLen | Opcode::SUpperRR | Opcode::SLowerRR => {
                if inst.operands.len() >= 2 {
                    if let OperandType::Register(r) = inst.operands[1] {
                        Some(r)
                    } else { None }
                } else { None }
            }
            _ => return false,
        };

        if let Some(r) = dst_reg {
            if r == r_ind {
                return false;
            }
            if !written_regs.contains(&r) {
                written_regs.push(r);
            }
        }
    }

    let base_reg = func.register_count;
    let regs_per_lane = written_regs.len() + 1;
    let total_new_regs = 3 * regs_per_lane + 1;
    let r_temp_limit = base_reg + 3 * regs_per_lane;
    func.register_count += total_new_regs;

    let mut new_loop_instrs = Vec::new();

    // 1. Vector Loop Header
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(3)),
            OperandType::Register(r_temp_limit),
        ],
    ));
    new_loop_instrs.push(Instruction::new(
        Opcode::ILtRRR,
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

    // Vector Body: Lane 0
    new_loop_instrs.extend(body.clone());

    // Lanes 1..4
    for k in 1..4 {
        let lane_base = base_reg + (k - 1) * regs_per_lane;
        let r_ind_k = lane_base;
        new_loop_instrs.push(Instruction::new(
            Opcode::IAddRCR,
            vec![
                OperandType::Register(r_ind),
                OperandType::Constant(Value::Int(k as i64)),
                OperandType::Register(r_ind_k),
            ],
        ));

        for inst in &body {
            let mut new_inst = inst.clone();
            for op in &mut new_inst.operands {
                if let OperandType::Register(r) = op {
                    if *r == r_ind {
                        *op = OperandType::Register(r_ind_k);
                    } else if let Some(idx) = written_regs.iter().position(|w| w == r) {
                        *op = OperandType::Register(lane_base + 1 + idx);
                    }
                }
            }
            new_loop_instrs.push(new_inst);
        }
    }

    // Vector Latch:
    new_loop_instrs.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(r_ind),
            OperandType::Constant(Value::Int(4)),
            OperandType::Register(r_ind),
        ],
    ));
    let vec_back_offset = -(new_loop_instrs.len() as i64);
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(vec_back_offset)],
    ));

    // Epilogue Header
    let epilogue_head_offset = (new_loop_instrs.len() - vec_cond_idx) as i64;
    new_loop_instrs[vec_cond_idx].operands[1] = OperandType::Offset(epilogue_head_offset);

    let epilogue_head_idx = new_loop_instrs.len();
    new_loop_instrs.push(Instruction::new(
        Opcode::ILtRRR,
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
    let epilogue_back_offset = -( (epi_latch_idx - epilogue_head_idx) as i64 );
    new_loop_instrs.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(epilogue_back_offset)],
    ));

    let final_exit_offset = (new_loop_instrs.len() - epilogue_cond_idx) as i64;
    new_loop_instrs[epilogue_cond_idx].operands[1] = OperandType::Offset(final_exit_offset);

    let old_loop_len = back_pc - head_pc + 1;
    let new_loop_len = new_loop_instrs.len();
    let diff = new_loop_len as i64 - old_loop_len as i64;

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
