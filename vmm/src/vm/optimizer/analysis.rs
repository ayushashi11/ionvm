//! CFG-aware type analysis, register facts, and scalar opcode specialization.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::bytecode::instruction::{Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};

// The analysis stores a type-state snapshot at each instruction. If the product
// would be excessive (e.g. malformed bytecode with huge register IDs), disable
// type-driven optimizations instead of risking an optimizer-induced OOM.
pub(super) const MAX_TYPE_FACT_CELLS: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct TypeAnalysis {
    /// Type facts immediately before each instruction. `None` means unreachable.
    pub(super) before: Vec<Option<Vec<ValType>>>,
    pub(super) inferred_return: Option<ValType>,
}

pub(super) fn value_type(value: &Value) -> ValType {
    match value {
        Value::Int(_) => ValType::Int,
        Value::Float(_) => ValType::Float,
        Value::Bool(_) => ValType::Bool,
        Value::String(_) => ValType::String,
        Value::Object(_) => ValType::Object,
        Value::Function(_) => ValType::Unknown,
        Value::Array(_) => ValType::Array,
        Value::IntArray(_) => ValType::IntArray,
        Value::FloatArray(_) => ValType::FloatArray,
        Value::V4Int(_) => ValType::V4Int,
        Value::V4Float(_) => ValType::V4Float,
        Value::V4Bool(_) => ValType::V4Bool,
        _ => ValType::Unknown,
    }
}

pub(super) fn operand_type(op: &OperandType, registers: &[ValType]) -> ValType {
    match op {
        OperandType::Register(r) => registers.get(*r).copied().unwrap_or(ValType::Unknown),
        OperandType::Argument(_) => ValType::Unknown,
        OperandType::Constant(v) => value_type(v),
        _ => ValType::Unknown,
    }
}

pub(super) fn operand_type_with_args(
    op: &OperandType,
    registers: &[ValType],
    argument_types: &[ValType],
) -> ValType {
    match op {
        OperandType::Argument(a) => argument_types.get(*a).copied().unwrap_or(ValType::Unknown),
        _ => operand_type(op, registers),
    }
}

pub(super) fn unknown_argument_types(func: &Function) -> Vec<ValType> {
    vec![ValType::Unknown; func.argument_count]
}

pub(super) fn register_count_needed(func: &Function) -> usize {
    fn visit_operand(op: &OperandType, max_reg: &mut Option<usize>) {
        match op {
            OperandType::Register(r) => {
                let current = *max_reg;
                *max_reg = Some(current.map_or(*r, |old| old.max(*r)));
            }
            OperandType::RegisterList(regs) => {
                for r in regs {
                    let current = *max_reg;
                    *max_reg = Some(current.map_or(*r, |old| old.max(*r)));
                }
            }
            OperandType::PayloadKeyList(ops) => {
                for nested in ops {
                    visit_operand(nested, max_reg);
                }
            }
            _ => {}
        }
    }

    let mut max_reg = None;
    for inst in &func.instructions {
        for op in &inst.operands {
            visit_operand(op, &mut max_reg);
        }
    }
    func.register_count.max(max_reg.map_or(0, |r| r.saturating_add(1)))
}

pub(super) fn constant_int(op: &OperandType) -> Option<i64> {
    match op {
        OperandType::Constant(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

pub(super) fn is_register(op: Option<&OperandType>) -> Option<usize> {
    match op {
        Some(OperandType::Register(r)) => Some(*r),
        _ => None,
    }
}

pub(super) fn jump_offset_index(inst: &Instruction) -> Option<usize> {
    match inst.opcode {
        Opcode::Jump => Some(0),
        Opcode::JumpTrue | Opcode::JumpFalse | Opcode::IJumpTrueR | Opcode::IJumpFalseR => {
            Some(1)
        }
        Opcode::IGtJmpRR
        | Opcode::IGtJmpRC
        | Opcode::ILtJmpRR
        | Opcode::ILtJmpRC
        | Opcode::IEqJmpRR
        | Opcode::IEqJmpRC
        | Opcode::FGtJmpRR
        | Opcode::FGtJmpRC
        | Opcode::FLtJmpRR
        | Opcode::FLtJmpRC
        | Opcode::ETagJmpRC => inst.operands.len().checked_sub(1),
        _ => None,
    }
}

pub(super) fn is_conditional_branch(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::JumpTrue
            | Opcode::JumpFalse
            | Opcode::IJumpTrueR
            | Opcode::IJumpFalseR
            | Opcode::IGtJmpRR
            | Opcode::IGtJmpRC
            | Opcode::ILtJmpRR
            | Opcode::ILtJmpRC
            | Opcode::IEqJmpRR
            | Opcode::IEqJmpRC
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::ETagJmpRC
    )
}

pub(super) fn target_from_offset(pc: usize, offset: i64, len: usize) -> Option<usize> {
    let target = (pc as i64).checked_add(offset)?;
    if target < 0 || target > len as i64 {
        None
    } else {
        Some(target as usize)
    }
}

pub(super) fn successors(func: &Function, pc: usize) -> Vec<usize> {
    let len = func.instructions.len();
    if pc >= len {
        return Vec::new();
    }
    let inst = &func.instructions[pc];
    if inst.opcode == Opcode::Ret {
        return Vec::new();
    }

    let mut out = Vec::new();
    if inst.opcode == Opcode::Jump {
        if let Some(OperandType::Offset(off)) = inst.operands.first() {
            if let Some(target) = target_from_offset(pc, *off, len) {
                if target < len {
                    out.push(target);
                }
            }
        }
        return out;
    }

    if inst.opcode == Opcode::ETagSwitch {
        for op in &inst.operands {
            if let OperandType::JumpTable(cases) = op {
                for case in cases {
                    if let Some(target) = target_from_offset(pc, case.offset, len) {
                        if target < len && !out.contains(&target) {
                            out.push(target);
                        }
                    }
                }
            }
        }
        if pc + 1 < len && !out.contains(&(pc + 1)) {
            out.push(pc + 1);
        }
        return out;
    }

    if is_conditional_branch(inst.opcode) {
        if let Some(idx) = jump_offset_index(inst) {
            if let Some(OperandType::Offset(off)) = inst.operands.get(idx) {
                if let Some(target) = target_from_offset(pc, *off, len) {
                    if target < len {
                        out.push(target);
                    }
                }
            }
        }
    }
    if pc + 1 < len && !out.contains(&(pc + 1)) {
        out.push(pc + 1);
    }
    out
}

pub(super) fn type_for_binary(op: Opcode, left: ValType, right: ValType) -> ValType {
    use ValType::*;
    match op {
        Opcode::Add => match (left, right) {
            (Int, Int) => Int,
            (Float, Float) => Float,
            (String, String) => String,
            (IntArray, IntArray) => IntArray,
            (FloatArray, FloatArray) => FloatArray,
            _ => Unknown,
        },
        Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem => match (left, right) {
            (Int, Int) => Int,
            (Float, Float) => Float,
            (IntArray, IntArray) => IntArray,
            (FloatArray, FloatArray) => FloatArray,
            _ => Unknown,
        },
        Opcode::Eq
        | Opcode::Lt
        | Opcode::Gt
        | Opcode::Le
        | Opcode::Ge
        | Opcode::Neq => Bool,
        Opcode::VCmpEqI | Opcode::VCmpGtI | Opcode::VCmpLtI => V4Int,
        Opcode::And | Opcode::Or | Opcode::Xor => match (left, right) {
            (Int, Int) => Int,
            (Bool, Bool) => Bool,
            _ => Unknown,
        },
        Opcode::IAddRRR
        | Opcode::IAddRCR
        | Opcode::ISubRRR
        | Opcode::ISubRCR
        | Opcode::ISubCRR
        | Opcode::IMulRRR
        | Opcode::IMulRCR
        | Opcode::IDivRCR
        | Opcode::IDivCRR
        | Opcode::IRemRCR
        | Opcode::IRemCRR
        | Opcode::IAndRRR
        | Opcode::IAndRCR
        | Opcode::IOrRRR
        | Opcode::IOrRCR
        | Opcode::IXorRRR
        | Opcode::IXorRCR
        | Opcode::IShlRRR
        | Opcode::IShlRCR
        | Opcode::IShrRRR
        | Opcode::IShrRCR => Int,
        Opcode::IEqRRR
        | Opcode::IEqRCR
        | Opcode::ILtRRR
        | Opcode::ILtRCR
        | Opcode::ILtCRR
        | Opcode::IGtRRR
        | Opcode::IGtRCR
        | Opcode::IGtCRR
        | Opcode::FEqRRR
        | Opcode::FEqRCR
        | Opcode::FLtRRR
        | Opcode::FLtRCR
        | Opcode::FLtCRR
        | Opcode::FGtRRR
        | Opcode::FGtRCR
        | Opcode::FGtCRR
        | Opcode::SEqRRR => Bool,
        Opcode::FAddRRR
        | Opcode::FAddRCR
        | Opcode::FSubRRR
        | Opcode::FSubRCR
        | Opcode::FSubCRR
        | Opcode::FMulRRR
        | Opcode::FMulRCR
        | Opcode::FDivRRR
        | Opcode::FDivRCR
        | Opcode::FDivCRR => Float,
        Opcode::SConcatRRR => String,
        Opcode::VAddI | Opcode::VSubI | Opcode::VMulI | Opcode::VDivI | Opcode::VRemI => {
            if left == IntArray && right == IntArray {
                IntArray
            } else if left == V4Int && right == V4Int {
                V4Int
            } else {
                Unknown
            }
        }
        Opcode::VAddIC | Opcode::VSubIC | Opcode::VMulIC | Opcode::VDivIC | Opcode::VRemIC => {
            if left == IntArray {
                IntArray
            } else if left == V4Int {
                V4Int
            } else {
                Unknown
            }
        }
        Opcode::VAddF | Opcode::VSubF | Opcode::VMulF | Opcode::VDivF => {
            if left == FloatArray && right == FloatArray {
                FloatArray
            } else if left == V4Float && right == V4Float {
                V4Float
            } else {
                Unknown
            }
        }
        _ => Unknown,
    }
}

pub(super) fn infer_instruction_write(
    inst: &Instruction,
    state: &[ValType],
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) -> Option<(usize, ValType)> {
    let ops = &inst.operands;
    let t0 = ops.first().map(|o| operand_type_with_args(o, state, argument_types)).unwrap_or(ValType::Unknown);
    let t1 = ops.get(1).map(|o| operand_type_with_args(o, state, argument_types)).unwrap_or(ValType::Unknown);

    match inst.opcode {
        Opcode::LoadConst => {
            let dst = is_register(ops.get(1))?;
            Some((dst, ops.first().map(|o| operand_type_with_args(o, state, argument_types)).unwrap_or(ValType::Unknown)))
        }
        Opcode::Call | Opcode::CallJit => {
            let argc = match ops.get(1) {
                Some(OperandType::Offset(n)) if *n >= 0 => *n as usize,
                _ => return None,
            };
            let dst = is_register(ops.get(2usize.checked_add(argc)?))?;
            let ret = match ops.first() {
                Some(OperandType::Function(name)) => returns
                    .get(name)
                    .copied()
                    .or_else(|| crate::builtin::get_native_return_type(name))
                    .unwrap_or(ValType::Unknown),
                _ => ValType::Unknown,
            };
            Some((dst, ret))
        }
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Div
        | Opcode::Rem
        | Opcode::Eq
        | Opcode::Lt
        | Opcode::Gt
        | Opcode::Le
        | Opcode::Ge
        | Opcode::Neq
        | Opcode::And
        | Opcode::Or
        | Opcode::Xor
        | Opcode::IAddRRR
        | Opcode::IAddRCR
        | Opcode::ISubRRR
        | Opcode::ISubRCR
        | Opcode::ISubCRR
        | Opcode::IMulRRR
        | Opcode::IMulRCR
        | Opcode::IDivRCR
        | Opcode::IDivCRR
        | Opcode::IRemRCR
        | Opcode::IRemCRR
        | Opcode::IEqRRR
        | Opcode::IEqRCR
        | Opcode::ILtRRR
        | Opcode::ILtRCR
        | Opcode::ILtCRR
        | Opcode::IGtRRR
        | Opcode::IGtRCR
        | Opcode::IGtCRR
        | Opcode::FAddRRR
        | Opcode::FAddRCR
        | Opcode::FSubRRR
        | Opcode::FSubRCR
        | Opcode::FSubCRR
        | Opcode::FMulRRR
        | Opcode::FMulRCR
        | Opcode::FDivRRR
        | Opcode::FDivRCR
        | Opcode::FDivCRR
        | Opcode::FEqRRR
        | Opcode::FEqRCR
        | Opcode::FLtRRR
        | Opcode::FLtRCR
        | Opcode::FLtCRR
        | Opcode::FGtRRR
        | Opcode::FGtRCR
        | Opcode::FGtCRR
        | Opcode::SEqRRR
        | Opcode::SConcatRRR
        | Opcode::IShlRRR
        | Opcode::IShlRCR
        | Opcode::IShrRRR
        | Opcode::IShrRCR
        | Opcode::IAndRRR
        | Opcode::IAndRCR
        | Opcode::IOrRRR
        | Opcode::IOrRCR
        | Opcode::IXorRRR
        | Opcode::IXorRCR
        | Opcode::VAddI
        | Opcode::VSubI
        | Opcode::VMulI
        | Opcode::VDivI
        | Opcode::VRemI
        | Opcode::VAddIC
        | Opcode::VSubIC
        | Opcode::VMulIC
        | Opcode::VDivIC
        | Opcode::VRemIC
        | Opcode::VAddF
        | Opcode::VSubF
        | Opcode::VMulF
        | Opcode::VDivF
        | Opcode::VCmpEqI
        | Opcode::VCmpGtI
        | Opcode::VCmpLtI => {
            let dst = is_register(ops.get(2))?;
            Some((dst, type_for_binary(inst.opcode, t0, t1)))
        }
        Opcode::Neg => {
            let dst = is_register(ops.get(1))?;
            let ty = if matches!(t0, ValType::Int | ValType::Float) { t0 } else { ValType::Unknown };
            Some((dst, ty))
        }
        Opcode::Not => Some((is_register(ops.get(1))?, ValType::Bool)),
        Opcode::GetProp => {
            let dst = is_register(ops.get(2))?;
            let is_length = matches!(ops.get(1), Some(OperandType::Property(p)) if p == "length")
                || matches!(ops.get(1), Some(OperandType::Constant(Value::String(s))) if s == "length");
            let ty = if is_length && matches!(t0, ValType::Array | ValType::IntArray | ValType::FloatArray) {
                ValType::Int
            } else {
                ValType::Unknown
            };
            Some((dst, ty))
        }
        Opcode::GetIndex | Opcode::ArrayGet => {
            let dst = is_register(ops.get(2))?;
            let ty = match t0 {
                ValType::IntArray => ValType::Int,
                ValType::FloatArray => ValType::Float,
                _ => ValType::Unknown,
            };
            Some((dst, ty))
        }
        Opcode::IArrayGet => Some((is_register(ops.get(2))?, ValType::Int)),
        Opcode::FArrayGet => Some((is_register(ops.get(2))?, ValType::Float)),
        Opcode::ArrayLen => Some((is_register(ops.get(1))?, ValType::Int)),
        Opcode::NewArray | Opcode::NewIntArray | Opcode::NewFloatArray => {
            let idx = if ops.len() >= 2 { 1 } else { 0 };
            let dst = is_register(ops.get(idx))?;
            let ty = match inst.opcode {
                Opcode::NewArray => ValType::Array,
                Opcode::NewIntArray => ValType::IntArray,
                Opcode::NewFloatArray => ValType::FloatArray,
                _ => ValType::Unknown,
            };
            Some((dst, ty))
        }
        Opcode::IBitGet => Some((is_register(ops.get(2))?, ValType::Int)),
        Opcode::SSliceRRR | Opcode::SUpperRR | Opcode::SLowerRR => {
            let dst_idx = if inst.opcode == Opcode::SSliceRRR { 2 } else { 1 };
            Some((is_register(ops.get(dst_idx))?, ValType::String))
        }
        Opcode::SSliceRRRR => Some((is_register(ops.get(3))?, ValType::String)),
        Opcode::SFindRRR | Opcode::SCountByteRRR => Some((is_register(ops.get(2))?, ValType::Int)),
        Opcode::SToIntRR => Some((is_register(ops.get(1))?, ValType::Int)),
        Opcode::VSelectI => Some((is_register(ops.get(3))?, ValType::V4Int)),
        Opcode::VSelectF => Some((is_register(ops.get(3))?, ValType::V4Float)),
        Opcode::VIotaI | Opcode::VSplatI => Some((is_register(ops.get(2))?, ValType::V4Int)),
        Opcode::VSplatF => Some((is_register(ops.get(2))?, ValType::V4Float)),
        Opcode::VGatherI => Some((is_register(ops.get(2))?, ValType::V4Int)),
        Opcode::VExtractI => Some((is_register(ops.get(2))?, ValType::Int)),
        Opcode::VSumI => Some((is_register(ops.get(1))?, ValType::Int)),
        Opcode::VSumF | Opcode::VDotF => Some((is_register(ops.get(1))?, ValType::Float)),
        _ => None,
    }
}

pub(super) fn preserves_register_types_without_writes(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Jump
            | Opcode::JumpTrue
            | Opcode::JumpFalse
            | Opcode::IJumpTrueR
            | Opcode::IJumpFalseR
            | Opcode::IGtJmpRR
            | Opcode::IGtJmpRC
            | Opcode::ILtJmpRR
            | Opcode::ILtJmpRC
            | Opcode::IEqJmpRR
            | Opcode::IEqJmpRC
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::ETagJmpRC
            | Opcode::ETagSwitch
            | Opcode::Ret
            | Opcode::Nop
            | Opcode::SetProp
            | Opcode::ArrayPush
            | Opcode::ArraySet
            | Opcode::IArraySet
            | Opcode::FArraySet
            | Opcode::Send
            // Spawn/Receive/ReceiveTimeout can write a result register. Their
            // operand schemas are not modeled here, so transfer_types() kills
            // all facts after them rather than preserving stale register types.
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0
    )
}

pub(super) fn transfer_types(inst: &Instruction, state: &mut [ValType], returns: &HashMap<String, ValType>, argument_types: &[ValType]) {
    if inst.opcode == Opcode::Swap {
        if let (Some(a), Some(b)) = (is_register(inst.operands.first()), is_register(inst.operands.get(1))) {
            if a < state.len() && b < state.len() {
                state.swap(a, b);
            }
        } else {
            state.fill(ValType::Unknown);
        }
        return;
    }

    if let Some((dst, ty)) = infer_instruction_write(inst, state, returns, argument_types) {
        if dst < state.len() {
            // Unknown is an actual analysis result, not permission to retain a
            // stale type from an earlier value in this register.
            state[dst] = ty;
        }
        return;
    }

    if preserves_register_types_without_writes(inst.opcode) {
        return;
    }

    // The instruction schema is not modeled here. Kill facts rather than risk
    // specializing a use based on a register value that this opcode replaced.
    state.fill(ValType::Unknown);
}

pub(super) fn merge_types(existing: &mut [ValType], incoming: &[ValType]) -> bool {
    let mut changed = false;
    for (old, new) in existing.iter_mut().zip(incoming.iter()) {
        if *old != *new && *old != ValType::Unknown {
            *old = ValType::Unknown;
            changed = true;
        }
    }
    changed
}

pub(super) fn analyze_types(func: &Function, returns: &HashMap<String, ValType>, argument_types: &[ValType]) -> TypeAnalysis {
    let len = func.instructions.len();
    let reg_count = register_count_needed(func);
    if len > 0 && reg_count > MAX_TYPE_FACT_CELLS / len {
        return TypeAnalysis { before: vec![None; len], inferred_return: None };
    }
    let mut before: Vec<Option<Vec<ValType>>> = vec![None; len];
    if len > 0 {
        before[0] = Some(vec![ValType::Unknown; reg_count]);
    }
    let mut queue = VecDeque::new();
    if len > 0 {
        queue.push_back(0usize);
    }

    while let Some(pc) = queue.pop_front() {
        let Some(in_state) = before[pc].clone() else { continue };
        let mut out_state = in_state;
        transfer_types(&func.instructions[pc], &mut out_state, returns, argument_types);
        for succ in successors(func, pc) {
            if succ >= len {
                continue;
            }
            match &mut before[succ] {
            Some(existing) => {
                if merge_types(existing, &out_state) {
                    queue.push_back(succ);
                }
            }
            slot @ None => {
                *slot = Some(out_state.clone());
                queue.push_back(succ);
            }
        }
        }
    }

    let mut found_return: Option<ValType> = None;
    let mut return_is_consistent = true;
    let mut saw_return = false;
    for (pc, inst) in func.instructions.iter().enumerate() {
        if inst.opcode != Opcode::Ret {
            continue;
        }
        let Some(state) = before.get(pc).and_then(|s| s.as_ref()) else { continue };
        saw_return = true;
        let ty = inst.operands.first().map(|op| operand_type_with_args(op, state, argument_types)).unwrap_or(ValType::Unknown);
        if ty == ValType::Unknown {
            return_is_consistent = false;
            continue;
        }
        match found_return {
            None => found_return = Some(ty),
            Some(prev) if prev == ty => {}
            Some(_) => return_is_consistent = false,
        }
    }
    let inferred_return = if saw_return && return_is_consistent { found_return } else { None };
    TypeAnalysis { before, inferred_return }
}

pub(super) fn swap_operands(inst: &mut Instruction, a: usize, b: usize) {
    if a < inst.operands.len() && b < inst.operands.len() {
        inst.operands.swap(a, b);
    }
}

pub(super) fn lower_binary_scalar(inst: &mut Instruction, left: ValType, right: ValType) -> bool {
    if inst.operands.len() < 3 || !matches!(inst.operands.get(2), Some(OperandType::Register(_))) {
        return false;
    }
    let old = inst.opcode;
    let a = inst.operands[0].clone();
    let b = inst.operands[1].clone();
    let a_reg = matches!(a, OperandType::Register(_));
    let b_reg = matches!(b, OperandType::Register(_));
    let a_int = constant_int(&a).is_some();
    let b_int = constant_int(&b).is_some();
    let a_float = matches!(a, OperandType::Constant(Value::Float(_)));
    let b_float = matches!(b, OperandType::Constant(Value::Float(_)));

    match old {
        Opcode::Add => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::IAddRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::IAddRCR; }
                else if a_int && b_reg { swap_operands(inst, 0, 1); inst.opcode = Opcode::IAddRCR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FAddRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FAddRCR; }
            } else if left == ValType::String && right == ValType::String && a_reg && b_reg {
                inst.opcode = Opcode::SConcatRRR;
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VAddI;
            } else if left == ValType::IntArray && b_int {
                inst.opcode = Opcode::VAddIC;
            } else if left == ValType::FloatArray && right == ValType::FloatArray {
                inst.opcode = Opcode::VAddF;
            }
        }
        Opcode::Sub => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::ISubRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::ISubRCR; }
                else if a_int && b_reg { inst.opcode = Opcode::ISubCRR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FSubRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FSubRCR; }
                else if a_float && b_reg { inst.opcode = Opcode::FSubCRR; }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VSubI;
            } else if left == ValType::IntArray && b_int {
                inst.opcode = Opcode::VSubIC;
            } else if left == ValType::FloatArray && right == ValType::FloatArray {
                inst.opcode = Opcode::VSubF;
            }
        }
        Opcode::Mul => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::IMulRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::IMulRCR; }
                else if a_int && b_reg { swap_operands(inst, 0, 1); inst.opcode = Opcode::IMulRCR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FMulRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FMulRCR; }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VMulI;
            } else if left == ValType::IntArray && b_int {
                inst.opcode = Opcode::VMulIC;
            } else if left == ValType::FloatArray && right == ValType::FloatArray {
                inst.opcode = Opcode::VMulF;
            }
        }
        Opcode::Div => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_int { inst.opcode = Opcode::IDivRCR; }
                else if a_int && b_reg { inst.opcode = Opcode::IDivCRR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FDivRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FDivRCR; }
                else if a_float && b_reg { inst.opcode = Opcode::FDivCRR; }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VDivI;
            } else if left == ValType::FloatArray && right == ValType::FloatArray {
                inst.opcode = Opcode::VDivF;
            }
        }
        Opcode::Rem => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_int { inst.opcode = Opcode::IRemRCR; }
                else if a_int && b_reg { inst.opcode = Opcode::IRemCRR; }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VRemI;
            } else if left == ValType::IntArray && b_int {
                inst.opcode = Opcode::VRemIC;
            }
        }
        Opcode::Eq => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::IEqRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::IEqRCR; }
                else if a_int && b_reg { swap_operands(inst, 0, 1); inst.opcode = Opcode::IEqRCR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FEqRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FEqRCR; }
                else if a_float && b_reg { swap_operands(inst, 0, 1); inst.opcode = Opcode::FEqRCR; }
            } else if left == ValType::String && right == ValType::String && a_reg && b_reg {
                inst.opcode = Opcode::SEqRRR;
            }
        }
        Opcode::Lt => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::ILtRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::ILtRCR; }
                else if a_int && b_reg { inst.opcode = Opcode::ILtCRR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FLtRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FLtRCR; }
                else if a_float && b_reg { inst.opcode = Opcode::FLtCRR; }
            }
        }
        Opcode::Gt => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg { inst.opcode = Opcode::IGtRRR; }
                else if a_reg && b_int { inst.opcode = Opcode::IGtRCR; }
                else if a_int && b_reg { inst.opcode = Opcode::IGtCRR; }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg { inst.opcode = Opcode::FGtRRR; }
                else if a_reg && b_float { inst.opcode = Opcode::FGtRCR; }
                else if a_float && b_reg { inst.opcode = Opcode::FGtCRR; }
            }
        }
        Opcode::And | Opcode::Or | Opcode::Xor => {
            if left == ValType::Int && right == ValType::Int {
                match (old, a_reg, b_reg, b_int) {
                    (Opcode::And, true, true, _) => inst.opcode = Opcode::IAndRRR,
                    (Opcode::And, true, false, true) => inst.opcode = Opcode::IAndRCR,
                    (Opcode::Or, true, true, _) => inst.opcode = Opcode::IOrRRR,
                    (Opcode::Or, true, false, true) => inst.opcode = Opcode::IOrRCR,
                    (Opcode::Xor, true, true, _) => inst.opcode = Opcode::IXorRRR,
                    (Opcode::Xor, true, false, true) => inst.opcode = Opcode::IXorRCR,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    inst.opcode != old
}

pub(super) fn specialize_instruction(inst: &mut Instruction, state: &[ValType]) -> bool {
    let old_opcode = inst.opcode;
    let old_operands = inst.operands.clone();
    let t0 = old_operands.first().map(|o| operand_type(o, state)).unwrap_or(ValType::Unknown);
    let t1 = old_operands.get(1).map(|o| operand_type(o, state)).unwrap_or(ValType::Unknown);

    let changed = match inst.opcode {
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem | Opcode::Eq | Opcode::Lt | Opcode::Gt | Opcode::And | Opcode::Or | Opcode::Xor => {
            lower_binary_scalar(inst, t0, t1)
        }
        Opcode::GetIndex | Opcode::ArrayGet => {
            if inst.operands.len() >= 3
                && is_register(inst.operands.get(1)).is_some()
                && matches!(inst.operands.get(2), Some(OperandType::Register(_)))
            {
                match t0 {
                    ValType::IntArray if t1 == ValType::Int => inst.opcode = Opcode::IArrayGet,
                    ValType::FloatArray if t1 == ValType::Int => inst.opcode = Opcode::FArrayGet,
                    _ => {}
                }
            }
            inst.opcode != old_opcode
        }
        Opcode::ArraySet => {
            if inst.operands.len() >= 3 && t1 == ValType::Int && is_register(inst.operands.get(1)).is_some() {
                let value_ty = operand_type(&inst.operands[2], state);
                match (t0, value_ty) {
                    (ValType::IntArray, ValType::Int) => inst.opcode = Opcode::IArraySet,
                    (ValType::FloatArray, ValType::Float) => inst.opcode = Opcode::FArraySet,
                    _ => {}
                }
            }
            inst.opcode != old_opcode
        }
        Opcode::GetProp => {
            let is_length = matches!(inst.operands.get(1), Some(OperandType::Property(p)) if p == "length")
                || matches!(inst.operands.get(1), Some(OperandType::Constant(Value::String(s))) if s == "length");
            if is_length
                && matches!(t0, ValType::Array | ValType::IntArray | ValType::FloatArray)
                && inst.operands.len() >= 3
            {
                let dst = inst.operands[2].clone();
                inst.opcode = Opcode::ArrayLen;
                inst.operands = vec![inst.operands[0].clone(), dst];
            }
            inst.opcode != old_opcode || inst.operands != old_operands
        }
        _ => false,
    };
    changed
}

pub(super) fn specialize_types(func: &mut Function, returns: &HashMap<String, ValType>, argument_types: &[ValType]) -> TypeAnalysis {
    let mut analysis = analyze_types(func, returns, argument_types);
    if func.return_type.is_none() {
        if let Some(ret) = analysis.inferred_return {
            func.return_type = Some(ret);
        }
    }

    // Each iteration only replaces a generic instruction with a more specific
    // opcode. A short fixed point lets the new result types inform later passes.
    for _ in 0..3 {
        analysis = analyze_types(func, returns, argument_types);
        let mut changed = false;
        for pc in 0..func.instructions.len() {
            let Some(state) = analysis.before.get(pc).and_then(|s| s.as_ref()) else { continue };
            changed |= specialize_instruction(&mut func.instructions[pc], state);
        }
        if !changed {
            break;
        }
    }
    analysis = analyze_types(func, returns, argument_types);
    if func.return_type.is_none() {
        if let Some(ret) = analysis.inferred_return {
            func.return_type = Some(ret);
        }
    }
    analysis
}

pub(super) fn operand_contains_register(op: &OperandType, needle: usize) -> bool {
    match op {
        OperandType::Register(r) => *r == needle,
        OperandType::RegisterList(regs) => regs.contains(&needle),
        OperandType::PayloadKeyList(ops) => ops.iter().any(|nested| operand_contains_register(nested, needle)),
        _ => false,
    }
}

pub(super) fn instruction_reads(inst: &Instruction) -> Vec<usize> {
    let mut out = Vec::new();
    if matches!(inst.opcode, Opcode::VSelectI | Opcode::VSelectF) {
        // The destination is also the false-value operand for in-place selects,
        // so only the first three operands are read.
        for op in inst.operands.iter().take(3) {
            if let OperandType::Register(r) = op { out.push(*r); }
        }
        return out;
    }
    let mut add = |op: Option<&OperandType>| {
        if let Some(OperandType::Register(r)) = op {
            out.push(*r);
        }
    };
    match inst.opcode {
        Opcode::LoadConst => add(inst.operands.first()),
        Opcode::IArrayGet | Opcode::FArrayGet | Opcode::ArrayGet | Opcode::GetIndex => {
            add(inst.operands.get(0)); add(inst.operands.get(1));
        }
        Opcode::IArraySet | Opcode::FArraySet | Opcode::ArraySet => {
            add(inst.operands.get(0)); add(inst.operands.get(1)); add(inst.operands.get(2));
        }
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem
        | Opcode::IAddRRR | Opcode::IAddRCR | Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR
        | Opcode::IMulRRR | Opcode::IMulRCR | Opcode::IDivRCR | Opcode::IDivCRR | Opcode::IRemRCR | Opcode::IRemCRR
        | Opcode::FAddRRR | Opcode::FAddRCR | Opcode::FSubRRR | Opcode::FSubRCR | Opcode::FSubCRR
        | Opcode::FMulRRR | Opcode::FMulRCR | Opcode::FDivRRR | Opcode::FDivRCR | Opcode::FDivCRR
        | Opcode::Eq | Opcode::Lt | Opcode::Gt | Opcode::IEqRRR | Opcode::IEqRCR | Opcode::ILtRRR
        | Opcode::ILtRCR | Opcode::ILtCRR | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::IGtCRR
        | Opcode::FEqRRR | Opcode::FEqRCR | Opcode::FLtRRR | Opcode::FLtRCR | Opcode::FLtCRR
        | Opcode::FGtRRR | Opcode::FGtRCR | Opcode::FGtCRR | Opcode::SConcatRRR
        | Opcode::IAndRRR | Opcode::IAndRCR | Opcode::IOrRRR | Opcode::IOrRCR | Opcode::IXorRRR | Opcode::IXorRCR
        | Opcode::IShlRRR | Opcode::IShlRCR | Opcode::IShrRRR | Opcode::IShrRCR => {
            add(inst.operands.get(0)); add(inst.operands.get(1));
        }
        Opcode::Neg | Opcode::Not => add(inst.operands.first()),
        _ => {
            for op in &inst.operands {
                if let OperandType::Register(r) = op { out.push(*r); }
            }
        }
    }
    out
}

pub(super) fn instruction_written_registers(inst: &Instruction) -> Vec<usize> {
    let mut out = Vec::new();
    if matches!(inst.opcode, Opcode::VSelectI | Opcode::VSelectF) {
        if let Some(OperandType::Register(r)) = inst.operands.get(3) { out.push(*r); }
        return out;
    }
    let binary = matches!(
        inst.opcode,
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem
        | Opcode::Eq | Opcode::Lt | Opcode::Gt | Opcode::Le | Opcode::Ge | Opcode::Neq
        | Opcode::And | Opcode::Or | Opcode::Xor
        | Opcode::IAddRRR | Opcode::IAddRCR | Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR
        | Opcode::IMulRRR | Opcode::IMulRCR | Opcode::IDivRCR | Opcode::IDivCRR | Opcode::IRemRCR | Opcode::IRemCRR
        | Opcode::IEqRRR | Opcode::IEqRCR | Opcode::ILtRRR | Opcode::ILtRCR | Opcode::ILtCRR
        | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::IGtCRR | Opcode::FAddRRR | Opcode::FAddRCR
        | Opcode::FSubRRR | Opcode::FSubRCR | Opcode::FSubCRR | Opcode::FMulRRR | Opcode::FMulRCR
        | Opcode::FDivRRR | Opcode::FDivRCR | Opcode::FDivCRR | Opcode::FEqRRR | Opcode::FEqRCR
        | Opcode::FLtRRR | Opcode::FLtRCR | Opcode::FLtCRR | Opcode::FGtRRR | Opcode::FGtRCR
        | Opcode::FGtCRR | Opcode::SEqRRR | Opcode::SConcatRRR | Opcode::IAndRRR | Opcode::IAndRCR
        | Opcode::IOrRRR | Opcode::IOrRCR | Opcode::IXorRRR | Opcode::IXorRCR | Opcode::IShlRRR
        | Opcode::IShlRCR | Opcode::IShrRRR | Opcode::IShrRCR | Opcode::IArrayGet | Opcode::FArrayGet
        | Opcode::ArrayGet | Opcode::GetIndex | Opcode::IBitGet | Opcode::SFindRRR | Opcode::SCountByteRRR
        | Opcode::VAddI | Opcode::VSubI | Opcode::VMulI | Opcode::VDivI | Opcode::VRemI | Opcode::VAddIC
        | Opcode::VSubIC | Opcode::VMulIC | Opcode::VDivIC | Opcode::VRemIC | Opcode::VAddF | Opcode::VSubF
        | Opcode::VMulF | Opcode::VDivF | Opcode::VGatherI | Opcode::VCmpEqI | Opcode::VCmpGtI | Opcode::VCmpLtI
    );
    if binary {
        if let Some(OperandType::Register(r)) = inst.operands.get(2) { out.push(*r); }
    } else {
        match inst.opcode {
            Opcode::LoadConst | Opcode::ArrayLen | Opcode::SToIntRR | Opcode::SUpperRR | Opcode::SLowerRR | Opcode::Neg | Opcode::Not => {
                if let Some(OperandType::Register(r)) = inst.operands.get(1) { out.push(*r); }
            }
            Opcode::SSliceRRR => {
                if let Some(OperandType::Register(r)) = inst.operands.get(2) { out.push(*r); }
            }
            Opcode::SSliceRRRR => {
                if let Some(OperandType::Register(r)) = inst.operands.get(3) { out.push(*r); }
            }
            Opcode::Call | Opcode::CallJit => {
                if let Some(OperandType::Offset(argc)) = inst.operands.get(1) {
                    if *argc >= 0 {
                        if let Some(OperandType::Register(r)) = inst.operands.get(2 + *argc as usize) { out.push(*r); }
                    }
                }
            }
            Opcode::VIotaI | Opcode::VSplatI | Opcode::VSplatF | Opcode::VExtractI => {
                if let Some(OperandType::Register(r)) = inst.operands.get(2) { out.push(*r); }
            }
            Opcode::VSumI | Opcode::VSumF | Opcode::VDotF => {
                if let Some(OperandType::Register(r)) = inst.operands.get(1) { out.push(*r); }
            }
            Opcode::NewArray | Opcode::NewIntArray | Opcode::NewFloatArray => {
                let idx = if inst.operands.len() >= 2 { 1 } else { 0 };
                if let Some(OperandType::Register(r)) = inst.operands.get(idx) { out.push(*r); }
            }
            _ => {}
        }
    }
    out
}

pub(super) fn instruction_mentions_register(inst: &Instruction, r: usize) -> bool {
    inst.operands.iter().any(|op| operand_contains_register(op, r))
}

pub(super) fn alloc_temp(next: &mut usize) -> usize {
    let reg = *next;
    *next = (*next).saturating_add(1);
    reg
}


pub(super) fn instruction_dominates_use(func: &Function, def_pc: usize, use_pc: usize) -> bool {
    if def_pc == use_pc || def_pc >= func.instructions.len() || use_pc >= func.instructions.len() {
        return false;
    }
    // A definition dominates a use iff the use is unreachable when that
    // definition is removed from the entry-reachable control-flow graph.
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([0usize]);
    while let Some(pc) = queue.pop_front() {
        if pc == def_pc || !visited.insert(pc) { continue; }
        if pc == use_pc { return false; }
        queue.extend(successors(func, pc));
    }
    true
}
