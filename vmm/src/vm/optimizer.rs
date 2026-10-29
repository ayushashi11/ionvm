//! Conservative bytecode optimizer for IonVM.
//!
//! Passes:
//! 1. CFG-aware type analysis and conservative scalar opcode specialization.
//! 2. Typed-array opcode selection based on the array's proven runtime type.
//! 3. Bounded scalar unrolling for tiny, statically counted loops with pure bodies.
//! 4. Four-lane SIMD for simple canonical integer-array loops and, in release
//!    builds, a conservative predicated CFG path for pure nested integer loops
//!    over read-only, proven `IntArray`s. Unsupported loops are left unchanged.
//!
//! Correctness policy: an optimization is only applied when its preconditions are
//! established by this module. Unknown types, unknown control-flow encodings,
//! possible loop-carried dependencies, possible array escapes, and unsupported
//! effects all cause a conservative bailout.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::bytecode::instruction::{Bytecode, Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};

const SIMD_WIDTH: i64 = 4;
// The analysis stores a type-state snapshot at each instruction. If the product
// would be excessive (e.g. malformed bytecode with huge register IDs), disable
// type-driven optimizations instead of risking an optimizer-induced OOM.
const MAX_TYPE_FACT_CELLS: usize = 16 * 1024 * 1024;

#[derive(Clone)]
struct TypeAnalysis {
    /// Type facts immediately before each instruction. `None` means unreachable.
    before: Vec<Option<Vec<ValType>>>,
    inferred_return: Option<ValType>,
}

fn value_type(value: &Value) -> ValType {
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

fn operand_type(op: &OperandType, registers: &[ValType]) -> ValType {
    match op {
        OperandType::Register(r) => registers.get(*r).copied().unwrap_or(ValType::Unknown),
        OperandType::Argument(_) => ValType::Unknown,
        OperandType::Constant(v) => value_type(v),
        _ => ValType::Unknown,
    }
}

fn operand_type_with_args(
    op: &OperandType,
    registers: &[ValType],
    argument_types: &[ValType],
) -> ValType {
    match op {
        OperandType::Argument(a) => argument_types.get(*a).copied().unwrap_or(ValType::Unknown),
        _ => operand_type(op, registers),
    }
}

fn unknown_argument_types(func: &Function) -> Vec<ValType> {
    vec![ValType::Unknown; func.argument_count]
}

fn register_count_needed(func: &Function) -> usize {
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
    func.register_count
        .max(max_reg.map_or(0, |r| r.saturating_add(1)))
}

fn constant_int(op: &OperandType) -> Option<i64> {
    match op {
        OperandType::Constant(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

fn is_register(op: Option<&OperandType>) -> Option<usize> {
    match op {
        Some(OperandType::Register(r)) => Some(*r),
        _ => None,
    }
}

fn jump_offset_index(inst: &Instruction) -> Option<usize> {
    match inst.opcode {
        Opcode::Jump => Some(0),
        Opcode::JumpTrue | Opcode::JumpFalse | Opcode::IJumpTrueR | Opcode::IJumpFalseR => Some(1),
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

fn is_conditional_branch(op: Opcode) -> bool {
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

fn target_from_offset(pc: usize, offset: i64, len: usize) -> Option<usize> {
    let target = (pc as i64).checked_add(offset)?;
    if target < 0 || target > len as i64 {
        None
    } else {
        Some(target as usize)
    }
}

fn successors(func: &Function, pc: usize) -> Vec<usize> {
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

fn type_for_binary(op: Opcode, left: ValType, right: ValType) -> ValType {
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
        Opcode::Eq | Opcode::Lt | Opcode::Gt | Opcode::Le | Opcode::Ge | Opcode::Neq => Bool,
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

fn infer_instruction_write(
    inst: &Instruction,
    state: &[ValType],
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) -> Option<(usize, ValType)> {
    let ops = &inst.operands;
    let t0 = ops
        .first()
        .map(|o| operand_type_with_args(o, state, argument_types))
        .unwrap_or(ValType::Unknown);
    let t1 = ops
        .get(1)
        .map(|o| operand_type_with_args(o, state, argument_types))
        .unwrap_or(ValType::Unknown);

    match inst.opcode {
        Opcode::LoadConst => {
            let dst = is_register(ops.get(1))?;
            Some((
                dst,
                ops.first()
                    .map(|o| operand_type_with_args(o, state, argument_types))
                    .unwrap_or(ValType::Unknown),
            ))
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
            let ty = if matches!(t0, ValType::Int | ValType::Float) {
                t0
            } else {
                ValType::Unknown
            };
            Some((dst, ty))
        }
        Opcode::Not => Some((is_register(ops.get(1))?, ValType::Bool)),
        Opcode::GetProp => {
            let dst = is_register(ops.get(2))?;
            let is_length = matches!(ops.get(1), Some(OperandType::Property(p)) if p == "length")
                || matches!(ops.get(1), Some(OperandType::Constant(Value::String(s))) if s == "length");
            let ty = if is_length
                && matches!(t0, ValType::Array | ValType::IntArray | ValType::FloatArray)
            {
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
            let dst_idx = if inst.opcode == Opcode::SSliceRRR {
                2
            } else {
                1
            };
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

fn preserves_register_types_without_writes(op: Opcode) -> bool {
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

fn transfer_types(
    inst: &Instruction,
    state: &mut [ValType],
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) {
    if inst.opcode == Opcode::Swap {
        if let (Some(a), Some(b)) = (
            is_register(inst.operands.first()),
            is_register(inst.operands.get(1)),
        ) {
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

fn merge_types(existing: &mut [ValType], incoming: &[ValType]) -> bool {
    let mut changed = false;
    for (old, new) in existing.iter_mut().zip(incoming.iter()) {
        if *old != *new && *old != ValType::Unknown {
            *old = ValType::Unknown;
            changed = true;
        }
    }
    changed
}

fn analyze_types(
    func: &Function,
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) -> TypeAnalysis {
    let len = func.instructions.len();
    let reg_count = register_count_needed(func);
    if len > 0 && reg_count > MAX_TYPE_FACT_CELLS / len {
        return TypeAnalysis {
            before: vec![None; len],
            inferred_return: None,
        };
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
        let Some(in_state) = before[pc].clone() else {
            continue;
        };
        let mut out_state = in_state;
        transfer_types(
            &func.instructions[pc],
            &mut out_state,
            returns,
            argument_types,
        );
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
        let Some(state) = before.get(pc).and_then(|s| s.as_ref()) else {
            continue;
        };
        saw_return = true;
        let ty = inst
            .operands
            .first()
            .map(|op| operand_type_with_args(op, state, argument_types))
            .unwrap_or(ValType::Unknown);
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
    let inferred_return = if saw_return && return_is_consistent {
        found_return
    } else {
        None
    };
    TypeAnalysis {
        before,
        inferred_return,
    }
}

fn swap_operands(inst: &mut Instruction, a: usize, b: usize) {
    if a < inst.operands.len() && b < inst.operands.len() {
        inst.operands.swap(a, b);
    }
}

fn lower_binary_scalar(inst: &mut Instruction, left: ValType, right: ValType) -> bool {
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
                if a_reg && b_reg {
                    inst.opcode = Opcode::IAddRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::IAddRCR;
                } else if a_int && b_reg {
                    swap_operands(inst, 0, 1);
                    inst.opcode = Opcode::IAddRCR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FAddRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FAddRCR;
                }
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
                if a_reg && b_reg {
                    inst.opcode = Opcode::ISubRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::ISubRCR;
                } else if a_int && b_reg {
                    inst.opcode = Opcode::ISubCRR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FSubRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FSubRCR;
                } else if a_float && b_reg {
                    inst.opcode = Opcode::FSubCRR;
                }
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
                if a_reg && b_reg {
                    inst.opcode = Opcode::IMulRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::IMulRCR;
                } else if a_int && b_reg {
                    swap_operands(inst, 0, 1);
                    inst.opcode = Opcode::IMulRCR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FMulRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FMulRCR;
                }
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
                if a_reg && b_int {
                    inst.opcode = Opcode::IDivRCR;
                } else if a_int && b_reg {
                    inst.opcode = Opcode::IDivCRR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FDivRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FDivRCR;
                } else if a_float && b_reg {
                    inst.opcode = Opcode::FDivCRR;
                }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VDivI;
            } else if left == ValType::FloatArray && right == ValType::FloatArray {
                inst.opcode = Opcode::VDivF;
            }
        }
        Opcode::Rem => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_int {
                    inst.opcode = Opcode::IRemRCR;
                } else if a_int && b_reg {
                    inst.opcode = Opcode::IRemCRR;
                }
            } else if left == ValType::IntArray && right == ValType::IntArray {
                inst.opcode = Opcode::VRemI;
            } else if left == ValType::IntArray && b_int {
                inst.opcode = Opcode::VRemIC;
            }
        }
        Opcode::Eq => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg {
                    inst.opcode = Opcode::IEqRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::IEqRCR;
                } else if a_int && b_reg {
                    swap_operands(inst, 0, 1);
                    inst.opcode = Opcode::IEqRCR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FEqRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FEqRCR;
                } else if a_float && b_reg {
                    swap_operands(inst, 0, 1);
                    inst.opcode = Opcode::FEqRCR;
                }
            } else if left == ValType::String && right == ValType::String && a_reg && b_reg {
                inst.opcode = Opcode::SEqRRR;
            }
        }
        Opcode::Lt => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg {
                    inst.opcode = Opcode::ILtRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::ILtRCR;
                } else if a_int && b_reg {
                    inst.opcode = Opcode::ILtCRR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FLtRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FLtRCR;
                } else if a_float && b_reg {
                    inst.opcode = Opcode::FLtCRR;
                }
            }
        }
        Opcode::Gt => {
            if left == ValType::Int && right == ValType::Int {
                if a_reg && b_reg {
                    inst.opcode = Opcode::IGtRRR;
                } else if a_reg && b_int {
                    inst.opcode = Opcode::IGtRCR;
                } else if a_int && b_reg {
                    inst.opcode = Opcode::IGtCRR;
                }
            } else if left == ValType::Float && right == ValType::Float {
                if a_reg && b_reg {
                    inst.opcode = Opcode::FGtRRR;
                } else if a_reg && b_float {
                    inst.opcode = Opcode::FGtRCR;
                } else if a_float && b_reg {
                    inst.opcode = Opcode::FGtCRR;
                }
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

fn specialize_instruction(inst: &mut Instruction, state: &[ValType]) -> bool {
    let old_opcode = inst.opcode;
    let old_operands = inst.operands.clone();
    let t0 = old_operands
        .first()
        .map(|o| operand_type(o, state))
        .unwrap_or(ValType::Unknown);
    let t1 = old_operands
        .get(1)
        .map(|o| operand_type(o, state))
        .unwrap_or(ValType::Unknown);

    let changed = match inst.opcode {
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Div
        | Opcode::Rem
        | Opcode::Eq
        | Opcode::Lt
        | Opcode::Gt
        | Opcode::And
        | Opcode::Or
        | Opcode::Xor => lower_binary_scalar(inst, t0, t1),
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
            if inst.operands.len() >= 3
                && t1 == ValType::Int
                && is_register(inst.operands.get(1)).is_some()
            {
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

fn specialize_types(
    func: &mut Function,
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) -> TypeAnalysis {
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
            let Some(state) = analysis.before.get(pc).and_then(|s| s.as_ref()) else {
                continue;
            };
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

fn operand_contains_register(op: &OperandType, needle: usize) -> bool {
    match op {
        OperandType::Register(r) => *r == needle,
        OperandType::RegisterList(regs) => regs.contains(&needle),
        OperandType::PayloadKeyList(ops) => ops
            .iter()
            .any(|nested| operand_contains_register(nested, needle)),
        _ => false,
    }
}

fn instruction_reads(inst: &Instruction) -> Vec<usize> {
    let mut out = Vec::new();
    if matches!(inst.opcode, Opcode::VSelectI | Opcode::VSelectF) {
        // The destination is also the false-value operand for in-place selects,
        // so only the first three operands are read.
        for op in inst.operands.iter().take(3) {
            if let OperandType::Register(r) = op {
                out.push(*r);
            }
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
            add(inst.operands.get(0));
            add(inst.operands.get(1));
        }
        Opcode::IArraySet | Opcode::FArraySet | Opcode::ArraySet => {
            add(inst.operands.get(0));
            add(inst.operands.get(1));
            add(inst.operands.get(2));
        }
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Div
        | Opcode::Rem
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
        | Opcode::Eq
        | Opcode::Lt
        | Opcode::Gt
        | Opcode::IEqRRR
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
        | Opcode::SConcatRRR
        | Opcode::IAndRRR
        | Opcode::IAndRCR
        | Opcode::IOrRRR
        | Opcode::IOrRCR
        | Opcode::IXorRRR
        | Opcode::IXorRCR
        | Opcode::IShlRRR
        | Opcode::IShlRCR
        | Opcode::IShrRRR
        | Opcode::IShrRCR => {
            add(inst.operands.get(0));
            add(inst.operands.get(1));
        }
        Opcode::Neg | Opcode::Not => add(inst.operands.first()),
        _ => {
            for op in &inst.operands {
                if let OperandType::Register(r) = op {
                    out.push(*r);
                }
            }
        }
    }
    out
}

fn instruction_written_registers(inst: &Instruction) -> Vec<usize> {
    let mut out = Vec::new();
    if matches!(inst.opcode, Opcode::VSelectI | Opcode::VSelectF) {
        if let Some(OperandType::Register(r)) = inst.operands.get(3) {
            out.push(*r);
        }
        return out;
    }
    let binary = matches!(
        inst.opcode,
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
            | Opcode::IAndRRR
            | Opcode::IAndRCR
            | Opcode::IOrRRR
            | Opcode::IOrRCR
            | Opcode::IXorRRR
            | Opcode::IXorRCR
            | Opcode::IShlRRR
            | Opcode::IShlRCR
            | Opcode::IShrRRR
            | Opcode::IShrRCR
            | Opcode::IArrayGet
            | Opcode::FArrayGet
            | Opcode::ArrayGet
            | Opcode::GetIndex
            | Opcode::IBitGet
            | Opcode::SFindRRR
            | Opcode::SCountByteRRR
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
            | Opcode::VGatherI
            | Opcode::VCmpEqI
            | Opcode::VCmpGtI
            | Opcode::VCmpLtI
    );
    if binary {
        if let Some(OperandType::Register(r)) = inst.operands.get(2) {
            out.push(*r);
        }
    } else {
        match inst.opcode {
            Opcode::LoadConst
            | Opcode::ArrayLen
            | Opcode::SToIntRR
            | Opcode::SUpperRR
            | Opcode::SLowerRR
            | Opcode::Neg
            | Opcode::Not => {
                if let Some(OperandType::Register(r)) = inst.operands.get(1) {
                    out.push(*r);
                }
            }
            Opcode::SSliceRRR => {
                if let Some(OperandType::Register(r)) = inst.operands.get(2) {
                    out.push(*r);
                }
            }
            Opcode::SSliceRRRR => {
                if let Some(OperandType::Register(r)) = inst.operands.get(3) {
                    out.push(*r);
                }
            }
            Opcode::Call | Opcode::CallJit => {
                if let Some(OperandType::Offset(argc)) = inst.operands.get(1) {
                    if *argc >= 0 {
                        if let Some(OperandType::Register(r)) =
                            inst.operands.get(2 + *argc as usize)
                        {
                            out.push(*r);
                        }
                    }
                }
            }
            Opcode::VIotaI | Opcode::VSplatI | Opcode::VSplatF | Opcode::VExtractI => {
                if let Some(OperandType::Register(r)) = inst.operands.get(2) {
                    out.push(*r);
                }
            }
            Opcode::VSumI | Opcode::VSumF | Opcode::VDotF => {
                if let Some(OperandType::Register(r)) = inst.operands.get(1) {
                    out.push(*r);
                }
            }
            Opcode::NewArray | Opcode::NewIntArray | Opcode::NewFloatArray => {
                let idx = if inst.operands.len() >= 2 { 1 } else { 0 };
                if let Some(OperandType::Register(r)) = inst.operands.get(idx) {
                    out.push(*r);
                }
            }
            _ => {}
        }
    }
    out
}

fn instruction_mentions_register(inst: &Instruction, r: usize) -> bool {
    inst.operands
        .iter()
        .any(|op| operand_contains_register(op, r))
}

fn alloc_temp(next: &mut usize) -> usize {
    let reg = *next;
    *next = (*next).saturating_add(1);
    reg
}

fn array_base_register(inst: &Instruction) -> Option<usize> {
    match inst.operands.first() {
        Some(OperandType::Register(r)) => Some(*r),
        _ => None,
    }
}

fn is_private_array_register(func: &Function, arr: usize, head: usize, back: usize) -> bool {
    // Require a local NewIntArray definition before the loop. This deliberately
    // excludes argument arrays and copied/shared references: the current VM has
    // no escape/alias metadata with which to prove those cases safe to reorder.
    let mut origin = None;
    for pc in 0..head {
        let inst = &func.instructions[pc];
        if instruction_written_registers(inst).contains(&arr) {
            origin = if inst.opcode == Opcode::NewIntArray
                && is_register(
                    inst.operands
                        .get(if inst.operands.len() >= 2 { 1 } else { 0 }),
                ) == Some(arr)
            {
                Some(pc)
            } else {
                None
            };
        }
    }
    let Some(origin_pc) = origin else {
        return false;
    };

    // Closure captures use a separate operand representation; the register-use
    // walker below cannot prove that a capture does not alias this array.
    if func
        .instructions
        .iter()
        .enumerate()
        .any(|(pc, inst)| (origin_pc..=back).contains(&pc) && inst.opcode == Opcode::MakeClosure)
    {
        return false;
    }

    // Before the loop, only direct array operations may refer to this array.
    // Any copy, call, send, property store, closure capture, or unknown use is an
    // escape and disables SIMD for this loop. Uses after the loop are harmless:
    // no concurrent observer can access the value before the function reaches
    // that later use unless it already escaped, which this check forbids.
    for (pc, inst) in func.instructions.iter().enumerate() {
        if pc == origin_pc || pc > back {
            continue;
        }
        if !instruction_mentions_register(inst, arr) {
            continue;
        }
        let direct_array_use = matches!(
            inst.opcode,
            Opcode::ArrayGet
                | Opcode::GetIndex
                | Opcode::IArrayGet
                | Opcode::ArraySet
                | Opcode::IArraySet
                | Opcode::ArrayLen
                | Opcode::ArrayPush
        ) && is_register(inst.operands.first()) == Some(arr)
            && !inst
                .operands
                .iter()
                .enumerate()
                .any(|(idx, op)| idx != 0 && operand_contains_register(op, arr));
        if !direct_array_use {
            return false;
        }
    }
    true
}

#[derive(Clone)]
enum StoreValue {
    Vector(usize),
    Scalar(OperandType),
}

#[derive(Clone)]
struct StorePlan {
    array: OperandType,
    value: StoreValue,
}

fn array_argument_origin_before(func: &Function, reg: usize, use_pc: usize) -> Option<usize> {
    fn recurse(
        func: &Function,
        reg: usize,
        use_pc: usize,
        seen: &mut HashSet<usize>,
    ) -> Option<usize> {
        if !seen.insert(reg) {
            return None;
        }
        let defs: Vec<(usize, &Instruction)> = func
            .instructions
            .iter()
            .enumerate()
            .filter(|(_, inst)| instruction_written_registers(inst).contains(&reg))
            .collect();
        if defs.len() != 1 {
            return None;
        }
        let (def_pc, inst) = defs[0];
        if def_pc >= use_pc
            || !instruction_dominates_use(func, def_pc, use_pc)
            || inst.opcode != Opcode::LoadConst
            || is_register(inst.operands.get(1)) != Some(reg)
        {
            return None;
        }
        match inst.operands.first()? {
            OperandType::Argument(arg) => Some(*arg),
            OperandType::Register(src) => recurse(func, *src, def_pc, seen),
            _ => None,
        }
    }
    recurse(func, reg, use_pc, &mut HashSet::new())
}

fn build_int_simd_prefix(
    func: &Function,
    head: usize,
    back: usize,
    types: &TypeAnalysis,
) -> Option<(Vec<Instruction>, usize)> {
    if head + 3 >= back || back == 0 || back >= func.instructions.len() {
        return None;
    }
    let instrs = &func.instructions;
    let head_inst = instrs.get(head)?;
    if !matches!(
        head_inst.opcode,
        Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR
    ) || head_inst.operands.len() < 3
    {
        return None;
    }
    let ind = is_register(head_inst.operands.first())?;
    let limit = head_inst.operands.get(1)?.clone();
    match head_inst.opcode {
        Opcode::Lt => {}
        Opcode::ILtRRR if matches!(&limit, OperandType::Register(_)) => {}
        Opcode::ILtRCR if matches!(&limit, OperandType::Constant(Value::Int(_))) => {}
        _ => return None,
    }
    // A negative constant limit is deliberately left scalar. For a dynamic
    // limit, a runtime guard below keeps all subsequent index arithmetic in the
    // non-negative range where `limit - ind` cannot overflow.
    if matches!(&limit, OperandType::Constant(Value::Int(v)) if *v < 0) {
        return None;
    }
    let cond = is_register(head_inst.operands.get(2))?;
    if ind == cond || matches!(&limit, OperandType::Register(r) if *r == cond) {
        return None;
    }
    let head_state = types.before.get(head)?.as_ref()?;
    if operand_type(&OperandType::Register(ind), head_state) != ValType::Int {
        return None;
    }
    match &limit {
        OperandType::Register(r) if head_state.get(*r).copied() != Some(ValType::Int) => {
            return None;
        }
        OperandType::Constant(Value::Int(_)) => {}
        _ => return None,
    }

    let branch = instrs.get(head + 1)?;
    if !matches!(branch.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR)
        || is_register(branch.operands.first()) != Some(cond)
    {
        return None;
    }
    let exit_off = match branch.operands.get(1) {
        Some(OperandType::Offset(off)) => *off,
        _ => return None,
    };
    let exit_target = target_from_offset(head + 1, exit_off, instrs.len())?;
    if exit_target <= back || exit_target > instrs.len() {
        return None;
    }

    let loop_back = instrs.get(back)?;
    if loop_back.opcode != Opcode::Jump {
        return None;
    }
    let back_off = match loop_back.operands.first() {
        Some(OperandType::Offset(off)) => *off,
        _ => return None,
    };
    if target_from_offset(back, back_off, instrs.len())? != head {
        return None;
    }

    let inc = instrs.get(back - 1)?;
    if !matches!(inc.opcode, Opcode::Add | Opcode::IAddRCR)
        || is_register(inc.operands.first()) != Some(ind)
        || inc.operands.get(1).and_then(constant_int) != Some(1)
        || is_register(inc.operands.get(2)) != Some(ind)
    {
        return None;
    }

    let body = &instrs[head + 2..back - 1];
    if body.is_empty() {
        return None;
    }
    let mut saw_store = false;
    let mut saw_store_yet = false;
    let mut arrays = BTreeSet::new();
    let mut body_defs = BTreeSet::new();

    // First structural/type validation. The body supports only operations whose
    // vector form has a precise scalar equivalent and no hidden side effects.
    for (offset, inst) in body.iter().enumerate() {
        let pc = head + 2 + offset;
        let state = types.before.get(pc)?.as_ref()?;
        match inst.opcode {
            Opcode::LoadConst => {
                if inst.operands.len() < 2
                    || !matches!(
                        inst.operands.first(),
                        Some(OperandType::Constant(Value::Int(_)))
                    )
                    || is_register(inst.operands.get(1)).is_none()
                {
                    return None;
                }
            }
            Opcode::IArrayGet => {
                if saw_store || inst.operands.len() < 3 {
                    return None;
                }
                let arr = array_base_register(inst)?;
                if state.get(arr).copied() != Some(ValType::IntArray)
                    || is_register(inst.operands.get(1)) != Some(ind)
                    || is_register(inst.operands.get(2)).is_none()
                {
                    return None;
                }
                arrays.insert(arr);
            }
            Opcode::IArraySet => {
                saw_store = true;
                saw_store_yet = true;
                if inst.operands.len() < 3 {
                    return None;
                }
                let arr = array_base_register(inst)?;
                if state.get(arr).copied() != Some(ValType::IntArray)
                    || is_register(inst.operands.get(1)) != Some(ind)
                {
                    return None;
                }
                arrays.insert(arr);
                match inst.operands.get(2)? {
                    OperandType::Constant(Value::Int(_)) => {}
                    OperandType::Register(r) if *r == ind => {}
                    OperandType::Register(r) if state.get(*r).copied() == Some(ValType::Int) => {}
                    _ => return None,
                }
            }
            Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR => {
                if inst.operands.len() < 3
                    || is_register(inst.operands.first()).is_none()
                    || is_register(inst.operands.get(1)).is_none()
                    || is_register(inst.operands.get(2)).is_none()
                {
                    return None;
                }
            }
            Opcode::IAddRCR | Opcode::ISubRCR | Opcode::IMulRCR => {
                if inst.operands.len() < 3
                    || is_register(inst.operands.first()).is_none()
                    || inst.operands.get(1).and_then(constant_int).is_none()
                    || is_register(inst.operands.get(2)).is_none()
                {
                    return None;
                }
            }
            Opcode::ISubCRR => {
                if inst.operands.len() < 3
                    || inst.operands.first().and_then(constant_int).is_none()
                    || is_register(inst.operands.get(1)).is_none()
                    || is_register(inst.operands.get(2)).is_none()
                {
                    return None;
                }
            }
            _ => return None,
        }

        // Every computed scalar value inside the loop must be an integer.
        if matches!(
            inst.opcode,
            Opcode::IAddRRR
                | Opcode::ISubRRR
                | Opcode::IMulRRR
                | Opcode::IAddRCR
                | Opcode::ISubRCR
                | Opcode::ISubCRR
                | Opcode::IMulRCR
        ) {
            for op in inst.operands.iter().take(2) {
                match op {
                    OperandType::Constant(Value::Int(_)) => {}
                    OperandType::Register(r) if *r == ind => {}
                    OperandType::Register(r) if state.get(*r).copied() == Some(ValType::Int) => {}
                    _ => return None,
                }
            }
        }

        for dst in instruction_written_registers(inst) {
            if dst == ind || dst == cond || matches!(&limit, OperandType::Register(r) if *r == dst)
            {
                return None;
            }
            body_defs.insert(dst);
        }
        if saw_store_yet && inst.opcode == Opcode::IArrayGet {
            // Reads after stores can depend on previous iterations when array
            // references alias. Keep the entire loop scalar in that case.
            return None;
        }
    }
    if !saw_store || arrays.is_empty() {
        return None;
    }
    if body_defs.iter().any(|dst| arrays.contains(dst)) {
        // Array base registers must continue to hold their original array object
        // for every load/store in the original loop.
        return None;
    }

    // The loop condition register is true on scalar iterations. Since the SIMD
    // prefix uses a private condition register, reject bodies that inspect it.
    for inst in body {
        if instruction_mentions_register(inst, cond) {
            return None;
        }
    }

    // Reject loop-carried values: a body-defined register may not be read before
    // this iteration has produced its first definition.
    let mut defined_so_far = HashSet::new();
    for inst in body {
        for read in instruction_reads(inst) {
            if read != ind && body_defs.contains(&read) && !defined_so_far.contains(&read) {
                return None;
            }
        }
        for dst in instruction_written_registers(inst) {
            defined_so_far.insert(dst);
        }
    }

    // The final scalar values of temporaries are observable if a later
    // instruction references them. Do not vectorize unless every body-defined
    // register is dead after the loop; array stores are the observable results.
    for reg in &body_defs {
        if instrs
            .iter()
            .enumerate()
            .any(|(pc, inst)| pc > back && instruction_mentions_register(inst, *reg))
        {
            return None;
        }
    }

    // Stores are emitted after the vector computations. Preserve each scalar
    // store's point-in-body value by rejecting a store source that is overwritten
    // later in the body (otherwise multiple stores could accidentally observe the
    // final vector register value).
    for (store_idx, inst) in body.iter().enumerate() {
        if inst.opcode != Opcode::IArraySet {
            continue;
        }
        if let Some(OperandType::Register(source)) = inst.operands.get(2) {
            if *source != ind
                && body[store_idx + 1..]
                    .iter()
                    .any(|later| instruction_written_registers(later).contains(source))
            {
                return None;
            }
        }
    }

    // Local ownership is the current alias/escape proof. External arrays or
    // arrays copied/passed/stored before the loop are not reordered.
    for arr in &arrays {
        if !is_private_array_register(func, *arr, head, back) {
            return None;
        }
    }

    // The condition limit must remain invariant and the induction update must
    // not be touched by the body.
    if body
        .iter()
        .any(|inst| instruction_written_registers(inst).contains(&ind))
    {
        return None;
    }
    if let OperandType::Register(limit_reg) = &limit {
        if body
            .iter()
            .any(|inst| instruction_written_registers(inst).contains(limit_reg))
        {
            return None;
        }
    }

    // Confirm that every direct offset in the function is either a branch target
    // or the call-argument-count field. We will relocate all branch targets after
    // inserting the prefix.
    if !all_offsets_are_relocatable(func) {
        return None;
    }

    let mut next_reg = func.register_count.max(register_count_needed(func));
    let r_vec_index = alloc_temp(&mut next_reg);
    let r_remaining = alloc_temp(&mut next_reg);
    let r_vec_cond = alloc_temp(&mut next_reg);
    let r_array_len = alloc_temp(&mut next_reg);
    let r_guard = alloc_temp(&mut next_reg);
    let lane1 = alloc_temp(&mut next_reg);
    let lane2 = alloc_temp(&mut next_reg);
    let lane3 = alloc_temp(&mut next_reg);
    let r_scalar_extract = alloc_temp(&mut next_reg);
    let lane_indices = [ind, lane1, lane2, lane3];

    let mut vector_registers = HashMap::new();
    for dst in &body_defs {
        vector_registers.insert(*dst, alloc_temp(&mut next_reg));
    }

    let mut prefix = Vec::new();
    // SIMD index generation is only valid with a non-negative induction
    // variable. In that domain, the guarded `limit - ind` calculation below is
    // representable without signed overflow and proves every lane is in range.
    prefix.push(Instruction::new(
        Opcode::ILtRCR,
        vec![
            OperandType::Register(ind),
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(r_guard),
        ],
    ));
    let guard_negative_branch = prefix.len();
    prefix.push(Instruction::new(
        Opcode::IJumpTrueR,
        vec![OperandType::Register(r_guard), OperandType::Offset(0)],
    ));

    // A runtime negative bound also falls back to the original loop. It may
    // still execute iterations if the starting index is even more negative.
    if let OperandType::Register(limit_reg) = &limit {
        prefix.push(Instruction::new(
            Opcode::ILtRCR,
            vec![
                OperandType::Register(*limit_reg),
                OperandType::Constant(Value::Int(0)),
                OperandType::Register(r_guard),
            ],
        ));
        prefix.push(Instruction::new(
            Opcode::IJumpTrueR,
            vec![OperandType::Register(r_guard), OperandType::Offset(0)],
        ));
    }

    // Any array access beyond a typed-array length would have different timing or
    // failure behavior if gathered in a vector. Preflight every unique array; if
    // the requested limit exceeds any length, run the original scalar loop.
    for arr in &arrays {
        prefix.push(Instruction::new(
            Opcode::ArrayLen,
            vec![
                OperandType::Register(*arr),
                OperandType::Register(r_array_len),
            ],
        ));
        let limit_check = match &limit {
            OperandType::Register(limit_reg) => Instruction::new(
                Opcode::ILtRRR,
                vec![
                    OperandType::Register(r_array_len),
                    OperandType::Register(*limit_reg),
                    OperandType::Register(r_guard),
                ],
            ),
            OperandType::Constant(Value::Int(v)) => Instruction::new(
                Opcode::ILtRCR,
                vec![
                    OperandType::Register(r_array_len),
                    OperandType::Constant(Value::Int(*v)),
                    OperandType::Register(r_guard),
                ],
            ),
            _ => return None,
        };
        prefix.push(limit_check);
        prefix.push(Instruction::new(
            Opcode::IJumpTrueR,
            vec![OperandType::Register(r_guard), OperandType::Offset(0)],
        ));
        // Patched after the total prefix length is known.
    }

    let vector_head = prefix.len();
    // Avoid `ind + 3 < limit`: after a final full batch, `ind` may become
    // i64::MAX and the addition would wrap on the next header. With non-negative
    // ind/limit, `remaining = limit - ind` stays within [-i64::MAX, i64::MAX].
    match &limit {
        OperandType::Register(limit_reg) => prefix.push(Instruction::new(
            Opcode::ISubRRR,
            vec![
                OperandType::Register(*limit_reg),
                OperandType::Register(ind),
                OperandType::Register(r_remaining),
            ],
        )),
        OperandType::Constant(Value::Int(v)) => prefix.push(Instruction::new(
            Opcode::ISubCRR,
            vec![
                OperandType::Constant(Value::Int(*v)),
                OperandType::Register(ind),
                OperandType::Register(r_remaining),
            ],
        )),
        _ => return None,
    }
    prefix.push(Instruction::new(
        Opcode::IGtRCR,
        vec![
            OperandType::Register(r_remaining),
            OperandType::Constant(Value::Int(SIMD_WIDTH - 1)),
            OperandType::Register(r_vec_cond),
        ],
    ));
    let vector_exit_branch = prefix.len();
    prefix.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![OperandType::Register(r_vec_cond), OperandType::Offset(0)],
    ));
    prefix.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Register(ind),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(r_vec_index),
        ],
    ));
    for (lane, lane_reg) in [(1i64, lane1), (2, lane2), (3, lane3)] {
        prefix.push(Instruction::new(
            Opcode::VExtractI,
            vec![
                OperandType::Register(r_vec_index),
                OperandType::Constant(Value::Int(lane)),
                OperandType::Register(lane_reg),
            ],
        ));
    }

    let mut scalar_splats: HashMap<usize, usize> = HashMap::new();
    let mut vector_body = Vec::new();
    let mut store_plans = Vec::new();

    fn ensure_vector_operand(
        op: &OperandType,
        state: &[ValType],
        ind: usize,
        v_index: usize,
        vector_registers: &HashMap<usize, usize>,
        scalar_splats: &mut HashMap<usize, usize>,
        next_reg: &mut usize,
        emitted: &mut Vec<Instruction>,
    ) -> Option<OperandType> {
        match op {
            OperandType::Register(r) if *r == ind => Some(OperandType::Register(v_index)),
            OperandType::Register(r) if vector_registers.contains_key(r) => {
                Some(OperandType::Register(*vector_registers.get(r)?))
            }
            OperandType::Register(r) if state.get(*r).copied() == Some(ValType::Int) => {
                let v = if let Some(v) = scalar_splats.get(r) {
                    *v
                } else {
                    let v = alloc_temp(next_reg);
                    emitted.push(Instruction::new(
                        Opcode::VSplatI,
                        vec![
                            OperandType::Register(*r),
                            OperandType::Constant(Value::Int(SIMD_WIDTH)),
                            OperandType::Register(v),
                        ],
                    ));
                    scalar_splats.insert(*r, v);
                    v
                };
                Some(OperandType::Register(v))
            }
            OperandType::Constant(Value::Int(value)) => {
                let v = alloc_temp(next_reg);
                emitted.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        OperandType::Constant(Value::Int(*value)),
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(v),
                    ],
                ));
                Some(OperandType::Register(v))
            }
            _ => None,
        }
    }

    for (offset, inst) in body.iter().enumerate() {
        let pc = head + 2 + offset;
        let state = types.before.get(pc)?.as_ref()?;
        match inst.opcode {
            Opcode::LoadConst => {
                let dst = is_register(inst.operands.get(1))?;
                let vector_dst = *vector_registers.get(&dst)?;
                let source = inst.operands.first()?.clone();
                vector_body.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        source,
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(vector_dst),
                    ],
                ));
            }
            Opcode::IArrayGet => {
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                vector_body.push(Instruction::new(
                    Opcode::VGatherI,
                    vec![
                        inst.operands[0].clone(),
                        OperandType::Register(r_vec_index),
                        OperandType::Register(vector_dst),
                    ],
                ));
            }
            Opcode::IArraySet => {
                let value = inst.operands.get(2)?;
                let plan_value = match value {
                    OperandType::Constant(Value::Int(_)) => StoreValue::Scalar(value.clone()),
                    OperandType::Register(r) if *r == ind => StoreValue::Vector(r_vec_index),
                    OperandType::Register(r) if vector_registers.contains_key(r) => {
                        StoreValue::Vector(*vector_registers.get(r)?)
                    }
                    OperandType::Register(r) if state.get(*r).copied() == Some(ValType::Int) => {
                        StoreValue::Scalar(OperandType::Register(*r))
                    }
                    _ => return None,
                };
                store_plans.push(StorePlan {
                    array: inst.operands[0].clone(),
                    value: plan_value,
                });
            }
            Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR => {
                let a = ensure_vector_operand(
                    &inst.operands[0],
                    state,
                    ind,
                    r_vec_index,
                    &vector_registers,
                    &mut scalar_splats,
                    &mut next_reg,
                    &mut vector_body,
                )?;
                let b = ensure_vector_operand(
                    &inst.operands[1],
                    state,
                    ind,
                    r_vec_index,
                    &vector_registers,
                    &mut scalar_splats,
                    &mut next_reg,
                    &mut vector_body,
                )?;
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                let opcode = match inst.opcode {
                    Opcode::IAddRRR => Opcode::VAddI,
                    Opcode::ISubRRR => Opcode::VSubI,
                    Opcode::IMulRRR => Opcode::VMulI,
                    _ => unreachable!(),
                };
                vector_body.push(Instruction::new(
                    opcode,
                    vec![a, b, OperandType::Register(vector_dst)],
                ));
            }
            Opcode::IAddRCR | Opcode::ISubRCR | Opcode::IMulRCR => {
                let a = ensure_vector_operand(
                    &inst.operands[0],
                    state,
                    ind,
                    r_vec_index,
                    &vector_registers,
                    &mut scalar_splats,
                    &mut next_reg,
                    &mut vector_body,
                )?;
                let k = constant_int(inst.operands.get(1)?)?;
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                let opcode = match inst.opcode {
                    Opcode::IAddRCR => Opcode::VAddIC,
                    Opcode::ISubRCR => Opcode::VSubIC,
                    Opcode::IMulRCR => Opcode::VMulIC,
                    _ => unreachable!(),
                };
                vector_body.push(Instruction::new(
                    opcode,
                    vec![
                        a,
                        OperandType::Constant(Value::Int(k)),
                        OperandType::Register(vector_dst),
                    ],
                ));
            }
            Opcode::ISubCRR => {
                let k = constant_int(inst.operands.first()?)?;
                let a = ensure_vector_operand(
                    &inst.operands[1],
                    state,
                    ind,
                    r_vec_index,
                    &vector_registers,
                    &mut scalar_splats,
                    &mut next_reg,
                    &mut vector_body,
                )?;
                let constant_vector = alloc_temp(&mut next_reg);
                vector_body.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        OperandType::Constant(Value::Int(k)),
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(constant_vector),
                    ],
                ));
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                vector_body.push(Instruction::new(
                    Opcode::VSubI,
                    vec![
                        OperandType::Register(constant_vector),
                        a,
                        OperandType::Register(vector_dst),
                    ],
                ));
            }
            _ => return None,
        }
    }
    prefix.extend(vector_body);

    // Convert each vector store back to ordered scalar typed-array stores. Lane
    // indices are distinct (i, i+1, i+2, i+3), and all reads precede stores.
    for store in store_plans {
        for lane in 0..4usize {
            let scalar_value = match &store.value {
                StoreValue::Scalar(value) => value.clone(),
                StoreValue::Vector(vector_reg) => {
                    prefix.push(Instruction::new(
                        Opcode::VExtractI,
                        vec![
                            OperandType::Register(*vector_reg),
                            OperandType::Constant(Value::Int(lane as i64)),
                            OperandType::Register(r_scalar_extract),
                        ],
                    ));
                    OperandType::Register(r_scalar_extract)
                }
            };
            prefix.push(Instruction::new(
                Opcode::IArraySet,
                vec![
                    store.array.clone(),
                    OperandType::Register(lane_indices[lane]),
                    scalar_value,
                ],
            ));
        }
    }

    // Advance by four and branch back to the vector guard.
    prefix.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(ind),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(ind),
        ],
    ));
    let vector_back = prefix.len();
    prefix.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(vector_head as i64 - vector_back as i64)],
    ));

    // Every guard or failed vector-width test falls through to the original loop
    // condition/body, which executes the remaining 0..3 iterations scalar-wise.
    let prefix_len = prefix.len();
    for idx in [guard_negative_branch, vector_exit_branch] {
        prefix[idx].operands[1] = OperandType::Offset((prefix_len - idx) as i64);
    }
    // The array-length guards occur in pairs (ArrayLen + compare + branch). Patch
    // each IJumpTrueR emitted before the vector loop to the original scalar head.
    for idx in 0..vector_head {
        if matches!(prefix[idx].opcode, Opcode::IJumpTrueR) {
            prefix[idx].operands[1] = OperandType::Offset((prefix_len - idx) as i64);
        }
    }
    Some((prefix, next_reg))
}

fn offset_is_non_branch_metadata(opcode: Opcode, idx: usize) -> bool {
    match opcode {
        Opcode::Call | Opcode::Spawn => idx == 1, // argument count
        Opcode::CallJit => idx == 0 || idx == 1,  // function-table index, argument count
        _ => false,
    }
}

fn all_offsets_are_relocatable(func: &Function) -> bool {
    for inst in &func.instructions {
        for (idx, op) in inst.operands.iter().enumerate() {
            match op {
                OperandType::Offset(_) => {
                    if !offset_is_non_branch_metadata(inst.opcode, idx)
                        && jump_offset_index(inst) != Some(idx)
                    {
                        return false;
                    }
                }
                OperandType::JumpTable(_) if inst.opcode != Opcode::ETagSwitch => return false,
                _ => {}
            }
        }
    }
    true
}

fn remap_pc(old_pc: usize, insertion_pc: usize, amount: usize) -> usize {
    if old_pc >= insertion_pc {
        old_pc.saturating_add(amount)
    } else {
        old_pc
    }
}

fn relocate_instruction_offsets(
    inst: &mut Instruction,
    old_pc: usize,
    new_pc: usize,
    insertion_pc: usize,
    amount: usize,
    old_len: usize,
) -> bool {
    let branch_idx = jump_offset_index(inst);
    for (idx, op) in inst.operands.iter_mut().enumerate() {
        if let OperandType::Offset(off) = op {
            // Some opcodes use Offset for metadata rather than control flow.
            // Never reinterpret call indices or argument counts as PCs.
            if offset_is_non_branch_metadata(inst.opcode, idx) {
                continue;
            }
            if branch_idx != Some(idx) {
                return false;
            }
            let Some(old_target) = target_from_offset(old_pc, *off, old_len) else {
                return false;
            };
            let new_target = remap_pc(old_target, insertion_pc, amount);
            *off = new_target as i64 - new_pc as i64;
        }
        if let OperandType::JumpTable(cases) = op {
            if inst.opcode != Opcode::ETagSwitch {
                return false;
            }
            for case in cases.iter_mut() {
                let Some(old_target) = target_from_offset(old_pc, case.offset, old_len) else {
                    return false;
                };
                let new_target = remap_pc(old_target, insertion_pc, amount);
                case.offset = new_target as i64 - new_pc as i64;
            }
        }
    }
    true
}

fn insert_prefix_with_relocation(
    func: &mut Function,
    insertion_pc: usize,
    prefix: Vec<Instruction>,
    new_register_count: usize,
) -> bool {
    if prefix.is_empty()
        || insertion_pc > func.instructions.len()
        || !all_offsets_are_relocatable(func)
    {
        return false;
    }
    let old_len = func.instructions.len();
    let amount = prefix.len();
    let old = func.instructions.clone();
    let mut relocated = Vec::with_capacity(old_len + amount);
    // Instructions before the insertion point keep their PC, but any branch
    // from them to the shifted suffix must still be relocated.
    for (old_pc, mut inst) in old.iter().take(insertion_pc).cloned().enumerate() {
        if !relocate_instruction_offsets(&mut inst, old_pc, old_pc, insertion_pc, amount, old_len) {
            return false;
        }
        relocated.push(inst);
    }
    relocated.extend(prefix);
    for (old_pc, mut inst) in old.into_iter().enumerate().skip(insertion_pc) {
        let new_pc = remap_pc(old_pc, insertion_pc, amount);
        if !relocate_instruction_offsets(&mut inst, old_pc, new_pc, insertion_pc, amount, old_len) {
            return false;
        }
        relocated.push(inst);
    }
    func.instructions = relocated;
    func.register_count = new_register_count;
    true
}

/// Remove an exact load/copy-back round-trip while preserving instruction PCs.
/// The intervening jump must simply fall through to the copy-back, the temporary
/// must have no other references, and no other branch may enter the copy-back.
/// Keeping three Nops instead of deleting instructions preserves relative jumps.
fn remove_redundant_copy_roundtrips(func: &mut Function) -> bool {
    let len = func.instructions.len();
    if len < 3 {
        return false;
    }
    let mut incoming = HashMap::<usize, Vec<usize>>::new();
    for (pc, inst) in func.instructions.iter().enumerate() {
        if let Some(target) = direct_branch_target(inst, pc, len) {
            incoming.entry(target).or_default().push(pc);
        }
        for op in &inst.operands {
            if let OperandType::JumpTable(cases) = op {
                if inst.opcode != Opcode::ETagSwitch {
                    continue;
                }
                for case in cases {
                    if let Some(target) = target_from_offset(pc, case.offset, len) {
                        incoming.entry(target).or_default().push(pc);
                    }
                }
            }
        }
    }

    let mut changed = false;
    for pc in 0..(len - 2) {
        let a = &func.instructions[pc];
        let b = &func.instructions[pc + 1];
        let c = &func.instructions[pc + 2];
        if a.opcode != Opcode::LoadConst
            || b.opcode != Opcode::Jump
            || c.opcode != Opcode::LoadConst
        {
            continue;
        }
        let (Some(OperandType::Register(src)), Some(OperandType::Register(tmp))) =
            (a.operands.first(), a.operands.get(1))
        else {
            continue;
        };
        if src == tmp
            || is_register(c.operands.first()) != Some(*tmp)
            || is_register(c.operands.get(1)) != Some(*src)
        {
            continue;
        }
        if !matches!(b.operands.first(), Some(OperandType::Offset(1))) {
            continue;
        }
        // An external branch into either middle instruction would bypass part
        // of the copy sequence and can make the apparent round-trip observable.
        if incoming
            .get(&(pc + 1))
            .map(|sources| !sources.is_empty())
            .unwrap_or(false)
        {
            continue;
        }
        if incoming
            .get(&(pc + 2))
            .map(|sources| sources.iter().any(|source| *source != pc + 1))
            .unwrap_or(false)
        {
            continue;
        }
        // The temp may only be referenced by the two copies.
        let outside_temp_use = func
            .instructions
            .iter()
            .enumerate()
            .any(|(other_pc, inst)| {
                if (pc..=pc + 2).contains(&other_pc) {
                    return false;
                }
                instruction_mentions_register(inst, *tmp)
            });
        if outside_temp_use {
            continue;
        }
        func.instructions[pc] = Instruction::new(Opcode::Nop, vec![]);
        func.instructions[pc + 1] = Instruction::new(Opcode::Nop, vec![]);
        func.instructions[pc + 2] = Instruction::new(Opcode::Nop, vec![]);
        changed = true;
    }
    changed
}

const MAX_UNROLLED_TRIPS: i64 = 4;
const MAX_UNROLLED_BODY: usize = 32;
const MAX_UNROLL_ROUNDS: usize = 32;

fn is_pure_unroll_opcode(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Nop
            | Opcode::LoadConst
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Rem
            | Opcode::Eq
            | Opcode::Lt
            | Opcode::Gt
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Neg
            | Opcode::Not
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
            | Opcode::IAndRRR
            | Opcode::IAndRCR
            | Opcode::IOrRRR
            | Opcode::IOrRCR
            | Opcode::IXorRRR
            | Opcode::IXorRCR
            | Opcode::IShlRRR
            | Opcode::IShlRCR
            | Opcode::IShrRRR
            | Opcode::IShrRCR
            | Opcode::Jump
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
            | Opcode::Ret
    )
}

fn direct_branch_target(inst: &Instruction, pc: usize, len: usize) -> Option<usize> {
    let idx = jump_offset_index(inst)?;
    let OperandType::Offset(off) = inst.operands.get(idx)? else {
        return None;
    };
    target_from_offset(pc, *off, len)
}

fn has_external_entry_into_loop(func: &Function, head: usize, back: usize) -> bool {
    let len = func.instructions.len();
    for (pc, inst) in func.instructions.iter().enumerate() {
        if (head..=back).contains(&pc) {
            continue;
        }
        if let Some(target) = direct_branch_target(inst, pc, len) {
            if (head..=back).contains(&target) {
                return true;
            }
        }
        for op in &inst.operands {
            if let OperandType::JumpTable(cases) = op {
                if inst.opcode != Opcode::ETagSwitch {
                    return true;
                }
                for case in cases {
                    let Some(target) = target_from_offset(pc, case.offset, len) else {
                        return true;
                    };
                    if (head..=back).contains(&target) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Clone each iteration of a tiny statically-counted scalar loop. This is a
/// scalar unroll, not SIMD: iterations execute in program order using the same
/// registers, so recurrences and early returns retain their scalar semantics.
/// The body is restricted to pure scalar operations and intra-iteration control
/// flow because the VM currently has no general side-effect/effect metadata.
fn build_small_unroll(func: &Function, head: usize, back: usize) -> Option<Vec<Instruction>> {
    if head == 0 || back <= head + 2 || back >= func.instructions.len() {
        return None;
    }
    if back - head > MAX_UNROLLED_BODY {
        return None;
    }

    let head_inst = func.instructions.get(head)?;
    if !matches!(head_inst.opcode, Opcode::Lt | Opcode::ILtRCR) {
        return None;
    }
    let ind = is_register(head_inst.operands.first())?;
    let limit = constant_int(head_inst.operands.get(1)?)?;
    let cond = is_register(head_inst.operands.get(2))?;
    if ind == cond {
        return None;
    }

    // Only fold a loop whose induction variable is initialized immediately
    // before the condition and whose bound is a literal. This excludes hidden
    // control-flow entries and dynamically changing limits.
    let init = func.instructions.get(head - 1)?;
    if init.opcode != Opcode::LoadConst
        || init.operands.first().and_then(constant_int).is_none()
        || is_register(init.operands.get(1)) != Some(ind)
    {
        return None;
    }
    let start = init.operands.first().and_then(constant_int)?;
    if start >= limit {
        return None;
    }
    let trips_i128 = i128::from(limit) - i128::from(start);
    if trips_i128 <= 0 || trips_i128 > i128::from(MAX_UNROLLED_TRIPS) {
        return None;
    }
    // The scalar loop increments after each iteration. Require the final value
    // to fit so the rewrite cannot remove an overflow that the scalar loop sees.
    if i128::from(start) + trips_i128 > i128::from(i64::MAX) {
        return None;
    }
    let trips = trips_i128 as usize;

    let condition_branch = func.instructions.get(head + 1)?;
    if !matches!(
        condition_branch.opcode,
        Opcode::JumpFalse | Opcode::IJumpFalseR
    ) || is_register(condition_branch.operands.first()) != Some(cond)
    {
        return None;
    }
    let exit = direct_branch_target(condition_branch, head + 1, func.instructions.len())?;
    if exit != back + 1 {
        return None;
    }
    let back_inst = func.instructions.get(back)?;
    if back_inst.opcode != Opcode::Jump
        || direct_branch_target(back_inst, back, func.instructions.len())? != head
    {
        return None;
    }

    let inc = func.instructions.get(back - 1)?;
    if !matches!(inc.opcode, Opcode::Add | Opcode::IAddRCR)
        || is_register(inc.operands.first()) != Some(ind)
        || inc.operands.get(1).and_then(constant_int) != Some(1)
        || is_register(inc.operands.get(2)) != Some(ind)
    {
        return None;
    }

    // The removed loop comparison writes `cond = false` on normal exit. If the
    // condition register is live after the loop, keep the original loop so that
    // observable final value is preserved.
    if func
        .instructions
        .iter()
        .enumerate()
        .any(|(pc, inst)| pc > back && instruction_mentions_register(inst, cond))
    {
        return None;
    }

    let body_start = head + 2;
    let body_end = back; // Exclusive; includes the induction increment at back - 1.
    let body = &func.instructions[body_start..body_end];
    if body.is_empty()
        || body.len().checked_mul(trips)? > MAX_UNROLLED_BODY * MAX_UNROLLED_TRIPS as usize
    {
        return None;
    }
    if !all_offsets_are_relocatable(func) || has_external_entry_into_loop(func, head, back) {
        return None;
    }

    // The loop's comparison result and induction variable cannot be touched by
    // the body except for the required final increment. A dynamic condition
    // register is an internal temporary; the unrolled code removes its writes.
    for inst in &body[..body.len() - 1] {
        if !is_pure_unroll_opcode(inst.opcode) || instruction_mentions_register(inst, cond) {
            return None;
        }
        if instruction_written_registers(inst).contains(&ind) {
            return None;
        }
        if inst
            .operands
            .iter()
            .any(|op| matches!(op, OperandType::JumpTable(_)))
        {
            return None;
        }
    }

    let mut output = Vec::with_capacity(body.len() * trips);
    for _ in 0..trips {
        let copy_base = head + output.len();
        for (body_idx, original) in body.iter().enumerate() {
            let old_pc = body_start + body_idx;
            let new_pc = copy_base + body_idx;
            let mut inst = original.clone();
            if let Some(branch_idx) = jump_offset_index(&inst) {
                let OperandType::Offset(off) = inst.operands.get(branch_idx)? else {
                    return None;
                };
                let target = target_from_offset(old_pc, *off, func.instructions.len())?;
                // All body control flow must remain within this iteration. In
                // particular, jumps to the loop header/back edge are rejected.
                if target < body_start || target >= body_end {
                    return None;
                }
                let new_target = copy_base + (target - body_start);
                inst.operands[branch_idx] = OperandType::Offset(new_target as i64 - new_pc as i64);
            }
            output.push(inst);
        }
    }
    Some(output)
}

fn remap_target_for_replacement(
    target: usize,
    start: usize,
    end_inclusive: usize,
    replacement_len: usize,
) -> Option<usize> {
    if target < start {
        Some(target)
    } else if target > end_inclusive {
        let old_len = end_inclusive - start + 1;
        if replacement_len >= old_len {
            target.checked_add(replacement_len - old_len)
        } else {
            target.checked_sub(old_len - replacement_len)
        }
    } else {
        // A branch from outside the loop into the removed range is not legal for
        // this transformation; internal loop branches were cloned separately.
        None
    }
}

fn relocate_outside_for_replacement(
    inst: &mut Instruction,
    old_pc: usize,
    new_pc: usize,
    start: usize,
    end_inclusive: usize,
    replacement_len: usize,
    old_len: usize,
) -> bool {
    let branch_idx = jump_offset_index(inst);
    for (idx, op) in inst.operands.iter_mut().enumerate() {
        if let OperandType::Offset(off) = op {
            if matches!(inst.opcode, Opcode::Call | Opcode::CallJit) && idx == 1 {
                continue;
            }
            if branch_idx != Some(idx) {
                return false;
            }
            let Some(target) = target_from_offset(old_pc, *off, old_len) else {
                return false;
            };
            let Some(new_target) =
                remap_target_for_replacement(target, start, end_inclusive, replacement_len)
            else {
                return false;
            };
            *off = new_target as i64 - new_pc as i64;
        }
        if let OperandType::JumpTable(cases) = op {
            if inst.opcode != Opcode::ETagSwitch {
                return false;
            }
            for case in cases.iter_mut() {
                let Some(target) = target_from_offset(old_pc, case.offset, old_len) else {
                    return false;
                };
                let Some(new_target) =
                    remap_target_for_replacement(target, start, end_inclusive, replacement_len)
                else {
                    return false;
                };
                case.offset = new_target as i64 - new_pc as i64;
            }
        }
    }
    true
}

fn replace_loop_with_unrolled_body(
    func: &mut Function,
    head: usize,
    back: usize,
    replacement: Vec<Instruction>,
) -> bool {
    if !all_offsets_are_relocatable(func) || has_external_entry_into_loop(func, head, back) {
        return false;
    }
    let old_len = func.instructions.len();
    let replacement_len = replacement.len();
    let mut new_instructions = Vec::with_capacity(old_len - (back - head + 1) + replacement_len);
    for old_pc in 0..head {
        let mut inst = func.instructions[old_pc].clone();
        if !relocate_outside_for_replacement(
            &mut inst,
            old_pc,
            old_pc,
            head,
            back,
            replacement_len,
            old_len,
        ) {
            return false;
        }
        new_instructions.push(inst);
    }
    new_instructions.extend(replacement);
    for old_pc in back + 1..old_len {
        let mut inst = func.instructions[old_pc].clone();
        let Some(new_pc) = remap_target_for_replacement(old_pc, head, back, replacement_len) else {
            return false;
        };
        if !relocate_outside_for_replacement(
            &mut inst,
            old_pc,
            new_pc,
            head,
            back,
            replacement_len,
            old_len,
        ) {
            return false;
        }
        new_instructions.push(inst);
    }
    func.instructions = new_instructions;
    true
}

fn unroll_small_constant_loops(func: &mut Function) -> bool {
    let mut changed = false;
    for _ in 0..MAX_UNROLL_ROUNDS {
        let mut transformed = false;
        let len = func.instructions.len();
        for back in 1..len {
            let Some(OperandType::Offset(off)) = func.instructions[back].operands.first() else {
                continue;
            };
            if func.instructions[back].opcode != Opcode::Jump || *off >= 0 {
                continue;
            }
            let Some(head) = target_from_offset(back, *off, len) else {
                continue;
            };
            if head >= back {
                continue;
            }
            let Some(replacement) = build_small_unroll(func, head, back) else {
                continue;
            };
            if replace_loop_with_unrolled_body(func, head, back, replacement) {
                changed = true;
                transformed = true;
                break;
            }
        }
        if !transformed {
            break;
        }
    }
    changed
}

// Whole-bytecode proof that an IntArray argument is immutable for the lifetime
// of every directly-visible use. The analysis intentionally rejects escaped
// functions, unknown calls, indirect values, and ambiguous register origins.
fn op_aliases_array(
    op: &OperandType,
    alias_regs: &HashSet<usize>,
    alias_args: &HashSet<usize>,
) -> bool {
    match op {
        OperandType::Register(r) => alias_regs.contains(r),
        OperandType::Argument(a) => alias_args.contains(a),
        OperandType::RegisterList(regs) => regs.iter().any(|r| alias_regs.contains(r)),
        OperandType::PayloadKeyList(ops) => ops
            .iter()
            .any(|nested| op_aliases_array(nested, alias_regs, alias_args)),
        _ => false,
    }
}

fn is_array_read_opcode(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::IArrayGet
            | Opcode::FArrayGet
            | Opcode::ArrayGet
            | Opcode::GetIndex
            | Opcode::ArrayLen
    )
}

fn is_array_write_opcode(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::IArraySet
            | Opcode::FArraySet
            | Opcode::ArraySet
            | Opcode::ArrayPush
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0
    )
}

fn local_array_argument_is_readonly(func: &Function, arg_idx: usize) -> bool {
    let alias_args = HashSet::from([arg_idx]);
    let mut alias_regs = HashSet::<usize>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for inst in &func.instructions {
            if inst.opcode != Opcode::LoadConst {
                continue;
            }
            let Some(OperandType::Register(dst)) = inst.operands.get(1) else {
                continue;
            };
            let source_alias = inst
                .operands
                .first()
                .map(|op| op_aliases_array(op, &alias_regs, &alias_args))
                .unwrap_or(false);
            if source_alias && alias_regs.insert(*dst) {
                changed = true;
            }
        }
    }

    for inst in &func.instructions {
        let aliases: Vec<(usize, &OperandType)> = inst
            .operands
            .iter()
            .enumerate()
            .filter(|(_, op)| op_aliases_array(op, &alias_regs, &alias_args))
            .collect();
        if aliases.is_empty() {
            continue;
        }
        match inst.opcode {
            Opcode::LoadConst => {
                let source_alias = inst
                    .operands
                    .first()
                    .map(|op| op_aliases_array(op, &alias_regs, &alias_args))
                    .unwrap_or(false);
                let dest_alias = matches!(inst.operands.get(1), Some(OperandType::Register(r)) if alias_regs.contains(r));
                if !source_alias || !dest_alias {
                    return false;
                }
            }
            op if is_array_read_opcode(op) => {
                if !inst
                    .operands
                    .first()
                    .map(|op| op_aliases_array(op, &alias_regs, &alias_args))
                    .unwrap_or(false)
                    || inst
                        .operands
                        .iter()
                        .skip(1)
                        .any(|op| op_aliases_array(op, &alias_regs, &alias_args))
                {
                    return false;
                }
            }
            Opcode::Call | Opcode::CallJit | Opcode::Spawn => {
                // This is only a local-shape check. The interprocedural proof
                // below verifies that every callee formal receiving this array
                // is itself readonly and every call target is statically known.
                if direct_callee_name_placeholder(inst).is_none() {
                    return false;
                }
                let Some(OperandType::Offset(argc)) = inst.operands.get(1) else {
                    return false;
                };
                if *argc < 0 {
                    return false;
                }
                for (operand_idx, op) in aliases {
                    if operand_idx < 2
                        || operand_idx >= 2 + *argc as usize
                        || !op_aliases_array(op, &alias_regs, &alias_args)
                    {
                        return false;
                    }
                }
            }
            _ => return false,
        }
    }
    true
}

// Small helper so local_array_argument_is_readonly doesn't need Bytecode to
// resolve a CallJit function index. Named calls are okay here; CallJit is
// accepted by the full analysis only where its index can be resolved.
fn direct_callee_name_placeholder(inst: &Instruction) -> Option<&str> {
    match inst.opcode {
        Opcode::Call | Opcode::Spawn => match inst.operands.first()? {
            OperandType::Function(name) => Some(name.as_str()),
            _ => None,
        },
        Opcode::CallJit => match inst.operands.first()? {
            OperandType::Offset(_) => Some("<indexed-callee>"),
            _ => None,
        },
        _ => None,
    }
}

fn local_readonly_array_parameters(bytecode: &Bytecode) -> HashMap<String, HashSet<usize>> {
    let mut out = HashMap::new();
    for func in &bytecode.functions {
        let mut safe = HashSet::new();
        for arg in 0..func.argument_count {
            if local_array_argument_is_readonly(func, arg) {
                safe.insert(arg);
            }
        }
        out.insert(func.name.clone(), safe);
    }

    // Compute a greatest fixed point: a locally read-only argument remains
    // eligible only if every statically resolved call that receives it passes
    // it to another locally read-only formal. Cycles are allowed only when all
    // members remain locally pure; unknown call targets eliminate the proof.
    loop {
        let mut remove = Vec::<(String, usize)>::new();
        for func in &bytecode.functions {
            let safe_args = out.get(&func.name).cloned().unwrap_or_default();
            for arg_idx in safe_args {
                let mut aliases = HashSet::<usize>::new();
                let mut changed = true;
                while changed {
                    changed = false;
                    for inst in &func.instructions {
                        if inst.opcode != Opcode::LoadConst {
                            continue;
                        }
                        let Some(OperandType::Register(dst)) = inst.operands.get(1) else {
                            continue;
                        };
                        let source_alias = match inst.operands.first() {
                            Some(OperandType::Argument(a)) => *a == arg_idx,
                            Some(OperandType::Register(r)) => aliases.contains(r),
                            _ => false,
                        };
                        if source_alias && aliases.insert(*dst) {
                            changed = true;
                        }
                    }
                }
                let alias_args = HashSet::from([arg_idx]);
                let mut proven = true;
                for inst in &func.instructions {
                    if !matches!(inst.opcode, Opcode::Call | Opcode::CallJit | Opcode::Spawn) {
                        continue;
                    }
                    if !inst
                        .operands
                        .iter()
                        .any(|op| op_aliases_array(op, &aliases, &alias_args))
                    {
                        continue;
                    }
                    let Some(callee_name) = direct_callee_name(bytecode, inst) else {
                        proven = false;
                        break;
                    };
                    let Some(OperandType::Offset(argc)) = inst.operands.get(1) else {
                        proven = false;
                        break;
                    };
                    if *argc < 0 {
                        proven = false;
                        break;
                    }
                    for idx in 0..(*argc as usize) {
                        let Some(actual) = inst.operands.get(2 + idx) else {
                            proven = false;
                            break;
                        };
                        if op_aliases_array(actual, &aliases, &alias_args)
                            && !out
                                .get(&callee_name)
                                .map(|set| set.contains(&idx))
                                .unwrap_or(false)
                        {
                            proven = false;
                            break;
                        }
                    }
                    if !proven {
                        break;
                    }
                }
                if !proven {
                    remove.push((func.name.clone(), arg_idx));
                }
            }
        }
        if remove.is_empty() {
            break;
        }
        for (name, arg) in remove {
            if let Some(args) = out.get_mut(&name) {
                args.remove(&arg);
            }
        }
    }
    out
}

fn instruction_dominates_use(func: &Function, def_pc: usize, use_pc: usize) -> bool {
    if def_pc == use_pc || def_pc >= func.instructions.len() || use_pc >= func.instructions.len() {
        return false;
    }
    // A definition dominates a use iff the use is unreachable when that
    // definition is removed from the entry-reachable control-flow graph.
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([0usize]);
    while let Some(pc) = queue.pop_front() {
        if pc == def_pc || !visited.insert(pc) {
            continue;
        }
        if pc == use_pc {
            return false;
        }
        queue.extend(successors(func, pc));
    }
    true
}

fn unique_int_array_constant(
    bytecode: &Bytecode,
    wanted: &std::sync::Arc<std::sync::RwLock<Vec<i64>>>,
) -> bool {
    let mut count = 0usize;
    for func in &bytecode.functions {
        for inst in &func.instructions {
            for op in &inst.operands {
                if let OperandType::Constant(Value::IntArray(arr)) = op {
                    if std::sync::Arc::ptr_eq(arr, wanted) {
                        count += 1;
                    }
                }
            }
        }
    }
    count == 1
}

fn array_argument_origins_are_safe(
    bytecode: &Bytecode,
    returns: &HashMap<String, ValType>,
    argument_types: &HashMap<String, Vec<ValType>>,
    local_readonly: &HashMap<String, HashSet<usize>>,
) -> HashMap<String, HashSet<usize>> {
    fn function_index(bytecode: &Bytecode, name: &str) -> Option<usize> {
        bytecode.functions.iter().position(|f| f.name == name)
    }

    fn escaped_functions(bytecode: &Bytecode) -> HashSet<String> {
        let mut escaped = HashSet::new();
        for func in &bytecode.functions {
            for inst in &func.instructions {
                for (idx, op) in inst.operands.iter().enumerate() {
                    if let OperandType::Function(name) = op {
                        if !(idx == 0 && matches!(inst.opcode, Opcode::Call | Opcode::Spawn)) {
                            escaped.insert(name.clone());
                        }
                    }
                }
            }
        }
        escaped
    }

    fn formal_arg_safe(
        bytecode: &Bytecode,
        returns: &HashMap<String, ValType>,
        caller_arg_types: &HashMap<String, Vec<ValType>>,
        function_idx: usize,
        arg_idx: usize,
        local_readonly: &HashMap<String, HashSet<usize>>,
        escaped: &HashSet<String>,
        visiting: &mut HashSet<(usize, usize)>,
    ) -> bool {
        let Some(func) = bytecode.functions.get(function_idx) else {
            return false;
        };
        if function_idx == 0
            || func.name == "main"
            || escaped.contains(&func.name)
            || caller_arg_types
                .get(&func.name)
                .and_then(|a| a.get(arg_idx))
                .copied()
                != Some(ValType::IntArray)
            || !local_readonly
                .get(&func.name)
                .map(|s| s.contains(&arg_idx))
                .unwrap_or(false)
            || !visiting.insert((function_idx, arg_idx))
        {
            return false;
        }

        let mut saw_site = false;
        for (caller_idx, caller) in bytecode.functions.iter().enumerate() {
            let caller_args = caller_arg_types
                .get(&caller.name)
                .cloned()
                .unwrap_or_else(|| unknown_argument_types(caller));
            let caller_analysis = analyze_types(caller, returns, &caller_args);
            for (pc, inst) in caller.instructions.iter().enumerate() {
                if direct_callee_name(bytecode, inst).as_deref() != Some(func.name.as_str()) {
                    continue;
                }
                saw_site = true;
                let Some(actual) = inst.operands.get(2 + arg_idx) else {
                    visiting.remove(&(function_idx, arg_idx));
                    return false;
                };
                let actual_type = caller_analysis
                    .before
                    .get(pc)
                    .and_then(|s| s.as_ref())
                    .map(|state| operand_type_with_args(actual, state, &caller_args))
                    .unwrap_or(ValType::Unknown);
                if actual_type != ValType::IntArray
                    || !actual_origin_safe(
                        bytecode,
                        returns,
                        caller_arg_types,
                        caller_idx,
                        actual,
                        pc,
                        local_readonly,
                        escaped,
                        visiting,
                    )
                {
                    visiting.remove(&(function_idx, arg_idx));
                    return false;
                }
            }
        }
        visiting.remove(&(function_idx, arg_idx));
        saw_site
    }

    fn actual_origin_safe(
        bytecode: &Bytecode,
        returns: &HashMap<String, ValType>,
        argument_types: &HashMap<String, Vec<ValType>>,
        caller_idx: usize,
        actual: &OperandType,
        call_pc: usize,
        local_readonly: &HashMap<String, HashSet<usize>>,
        escaped: &HashSet<String>,
        visiting: &mut HashSet<(usize, usize)>,
    ) -> bool {
        let Some(caller) = bytecode.functions.get(caller_idx) else {
            return false;
        };
        match actual {
            OperandType::Argument(a) => formal_arg_safe(
                bytecode,
                returns,
                argument_types,
                caller_idx,
                *a,
                local_readonly,
                escaped,
                visiting,
            ),
            OperandType::Constant(Value::IntArray(arr)) => unique_int_array_constant(bytecode, arr),
            OperandType::Register(r) => {
                let defs: Vec<(usize, &Instruction)> = caller
                    .instructions
                    .iter()
                    .enumerate()
                    .filter(|(_, inst)| instruction_written_registers(inst).contains(r))
                    .collect();
                if defs.len() != 1 {
                    return false;
                }
                let (def_pc, def) = defs[0];
                if def_pc >= call_pc || !instruction_dominates_use(caller, def_pc, call_pc) {
                    return false;
                }
                match def.opcode {
                    Opcode::NewIntArray => {
                        let dest_idx = if def.operands.len() >= 2 { 1 } else { 0 };
                        if is_register(def.operands.get(dest_idx)) != Some(*r) {
                            return false;
                        }
                        local_array_lifetime_is_safe(
                            bytecode,
                            returns,
                            argument_types,
                            caller_idx,
                            *r,
                            def_pc,
                            local_readonly,
                            escaped,
                            visiting,
                        )
                    }
                    Opcode::LoadConst if is_register(def.operands.get(1)) == Some(*r) => {
                        let Some(source) = def.operands.first() else {
                            return false;
                        };
                        match source {
                            OperandType::Register(src) => actual_origin_safe(
                                bytecode,
                                returns,
                                argument_types,
                                caller_idx,
                                &OperandType::Register(*src),
                                def_pc,
                                local_readonly,
                                escaped,
                                visiting,
                            ),
                            OperandType::Argument(a) => formal_arg_safe(
                                bytecode,
                                returns,
                                argument_types,
                                caller_idx,
                                *a,
                                local_readonly,
                                escaped,
                                visiting,
                            ),
                            OperandType::Constant(Value::IntArray(arr))
                                if unique_int_array_constant(bytecode, arr) =>
                            {
                                local_array_lifetime_is_safe(
                                    bytecode,
                                    returns,
                                    argument_types,
                                    caller_idx,
                                    *r,
                                    def_pc,
                                    local_readonly,
                                    escaped,
                                    visiting,
                                )
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn local_array_lifetime_is_safe(
        bytecode: &Bytecode,
        returns: &HashMap<String, ValType>,
        argument_types: &HashMap<String, Vec<ValType>>,
        caller_idx: usize,
        root_reg: usize,
        origin_pc: usize,
        local_readonly: &HashMap<String, HashSet<usize>>,
        escaped: &HashSet<String>,
        visiting: &mut HashSet<(usize, usize)>,
    ) -> bool {
        let Some(caller) = bytecode.functions.get(caller_idx) else {
            return false;
        };
        let mut aliases = HashSet::from([root_reg]);
        let mut changed = true;
        while changed {
            changed = false;
            for (pc, inst) in caller.instructions.iter().enumerate().skip(origin_pc + 1) {
                if inst.opcode != Opcode::LoadConst {
                    continue;
                }
                let Some(OperandType::Register(dst)) = inst.operands.get(1) else {
                    continue;
                };
                if matches!(inst.operands.first(), Some(OperandType::Register(src)) if aliases.contains(src))
                    && aliases.insert(*dst)
                {
                    changed = true;
                }
                let _ = pc;
            }
        }
        // Every alias must have exactly one definition, and that definition
        // must dominate every read. Otherwise a register reuse or a branch that
        // bypasses initialization could make the runtime value unrelated.
        for alias in &aliases {
            let defs: Vec<(usize, &Instruction)> = caller
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, inst)| instruction_written_registers(inst).contains(alias))
                .collect();
            if defs.len() != 1 {
                return false;
            }
            let (def_pc, def) = defs[0];
            let valid_origin = def_pc == origin_pc
                && *alias == root_reg
                && ((def.opcode == Opcode::NewIntArray
                    && is_register(
                        def.operands
                            .get(if def.operands.len() >= 2 { 1 } else { 0 }),
                    ) == Some(*alias))
                    || (def.opcode == Opcode::LoadConst
                        && is_register(def.operands.get(1)) == Some(*alias)
                        && matches!(
                            def.operands.first(),
                            Some(OperandType::Constant(Value::IntArray(_)))
                        )));
            let valid_copy = def_pc > origin_pc
                && def.opcode == Opcode::LoadConst
                && is_register(def.operands.get(1)) == Some(*alias)
                && matches!(def.operands.first(), Some(OperandType::Register(src)) if aliases.contains(src));
            if !valid_origin && !valid_copy {
                return false;
            }
            for (use_pc, inst) in caller.instructions.iter().enumerate() {
                if use_pc == def_pc || !instruction_reads(inst).contains(alias) {
                    continue;
                }
                if !instruction_dominates_use(caller, def_pc, use_pc) {
                    return false;
                }
            }
        }

        let mut first_spawn: Option<usize> = None;
        for (pc, inst) in caller.instructions.iter().enumerate() {
            if inst.opcode != Opcode::Spawn {
                continue;
            }
            let Some(callee_name) = direct_callee_name(bytecode, inst) else {
                continue;
            };
            let Some(OperandType::Offset(argc)) = inst.operands.get(1) else {
                return false;
            };
            if *argc < 0 {
                return false;
            }
            for arg_idx in 0..(*argc as usize) {
                let Some(op) = inst.operands.get(2 + arg_idx) else {
                    return false;
                };
                if op_aliases_array(op, &aliases, &HashSet::new()) {
                    if !local_readonly
                        .get(&callee_name)
                        .map(|s| s.contains(&arg_idx))
                        .unwrap_or(false)
                    {
                        return false;
                    }
                    first_spawn = Some(first_spawn.map_or(pc, |old| old.min(pc)));
                }
            }
        }

        for (pc, inst) in caller.instructions.iter().enumerate() {
            let uses_alias = inst
                .operands
                .iter()
                .any(|op| op_aliases_array(op, &aliases, &HashSet::new()));
            let is_origin = pc == origin_pc
                && ((inst.opcode == Opcode::NewIntArray
                    && instruction_written_registers(inst).contains(&root_reg))
                    || (inst.opcode == Opcode::LoadConst
                        && is_register(inst.operands.get(1)) == Some(root_reg)
                        && matches!(
                            inst.operands.first(),
                            Some(OperandType::Constant(Value::IntArray(_)))
                        )));
            if is_origin {
                continue;
            }
            if !uses_alias {
                continue;
            }
            match inst.opcode {
                Opcode::LoadConst => {
                    let source_alias = inst
                        .operands
                        .first()
                        .map(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                        .unwrap_or(false);
                    let dest_alias = matches!(inst.operands.get(1), Some(OperandType::Register(r)) if aliases.contains(r));
                    if pc <= origin_pc || !source_alias || !dest_alias {
                        return false;
                    }
                }
                op if is_array_read_opcode(op) => {
                    if !inst
                        .operands
                        .first()
                        .map(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                        .unwrap_or(false)
                        || inst
                            .operands
                            .iter()
                            .skip(1)
                            .any(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                    {
                        return false;
                    }
                }
                op if is_array_write_opcode(op) => {
                    if !inst
                        .operands
                        .first()
                        .map(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                        .unwrap_or(false)
                        || inst
                            .operands
                            .iter()
                            .skip(1)
                            .any(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                        || first_spawn
                            .map(|shared_pc| pc >= shared_pc)
                            .unwrap_or(false)
                    {
                        return false;
                    }
                }
                Opcode::Call | Opcode::CallJit | Opcode::Spawn => {
                    let Some(callee_name) = direct_callee_name(bytecode, inst) else {
                        return false;
                    };
                    let Some(OperandType::Offset(argc)) = inst.operands.get(1) else {
                        return false;
                    };
                    if *argc < 0 {
                        return false;
                    }
                    let Some(arg_end) = 2usize.checked_add(*argc as usize) else {
                        return false;
                    };
                    // Every occurrence of this array alias in the instruction
                    // must be an actual argument. It may not be a call target,
                    // arity/table metadata, or a destination register.
                    for (operand_idx, op) in inst.operands.iter().enumerate() {
                        if op_aliases_array(op, &aliases, &HashSet::new())
                            && (operand_idx < 2 || operand_idx >= arg_end)
                        {
                            return false;
                        }
                    }
                    for arg_idx in 0..(*argc as usize) {
                        let Some(op) = inst.operands.get(2 + arg_idx) else {
                            return false;
                        };
                        if op_aliases_array(op, &aliases, &HashSet::new())
                            && !local_readonly
                                .get(&callee_name)
                                .map(|args| args.contains(&arg_idx))
                                .unwrap_or(false)
                        {
                            return false;
                        }
                    }
                }
                _ => return false,
            }
        }
        true
    }

    let escaped = escaped_functions(bytecode);
    let mut safe: HashMap<String, HashSet<usize>> = HashMap::new();
    for func in &bytecode.functions {
        let args = argument_types
            .get(&func.name)
            .cloned()
            .unwrap_or_else(|| unknown_argument_types(func));
        let Some(function_idx) = function_index(bytecode, &func.name) else {
            continue;
        };
        let mut safe_args = HashSet::new();
        for arg_idx in 0..func.argument_count {
            if args.get(arg_idx).copied() != Some(ValType::IntArray) {
                continue;
            }
            let mut visiting = HashSet::new();
            if formal_arg_safe(
                bytecode,
                returns,
                argument_types,
                function_idx,
                arg_idx,
                local_readonly,
                &escaped,
                &mut visiting,
            ) {
                safe_args.insert(arg_idx);
            }
        }
        safe.insert(func.name.clone(), safe_args);
    }
    safe
}

// General predicated CFG-to-SIMD lowering for pure integer loop regions. Each lane
// carries a virtual bytecode PC, so branches and nested loops remain lane-local.
// The outer loop's latch ends one candidate iteration; branch targets past the
// loop are admitted only when every path through that epilogue ends in Ret.
// This deliberately supports a small, explicitly audited opcode subset and
// requires a range proof for every gather index before it will emit VGatherI.
fn predicated_loop_region(func: &Function, head: usize, back: usize) -> Option<Vec<usize>> {
    let len = func.instructions.len();
    if head + 2 >= back || back >= len || back == 0 {
        return None;
    }
    let body_start = head + 2;
    let latch = back - 1;
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([body_start]);

    while let Some(pc) = queue.pop_front() {
        if pc == latch {
            seen.insert(pc);
            continue;
        }
        // The outer back-edge itself means this candidate's scalar iteration
        // has completed. SIMD has a separate induction value per lane, so this
        // edge maps to the vectorized "done" PC instead of executing the jump.
        if pc == back {
            continue;
        }
        if pc < body_start || pc == head || pc == head + 1 || pc >= len {
            return None;
        }
        if !seen.insert(pc) {
            continue;
        }
        if seen.len() > 160 {
            return None;
        }
        if func.instructions[pc].opcode == Opcode::Ret {
            continue;
        }
        for next in successors(func, pc) {
            if next == latch {
                seen.insert(latch);
            } else if next == back {
                // A branch can explicitly target the outer back-edge. It is a
                // terminal state for that SIMD lane, not an external side exit.
                continue;
            } else if next < body_start || next == head || next == head + 1 || next >= len {
                return None;
            } else if !seen.contains(&next) {
                queue.push_back(next);
            }
        }
    }

    if !seen.contains(&latch)
        || !seen
            .iter()
            .any(|pc| func.instructions[*pc].opcode == Opcode::Ret)
        || !seen.contains(&body_start)
    {
        return None;
    }

    // Some compilers encode an early return through a small epilogue after the
    // loop's back-edge. Permit it only if every path is a pure acyclic path
    // terminating in Ret. No path may return to the candidate loop or continue
    // into arbitrary post-loop code. The later opcode legality pass performs
    // the side-effect and type checks for every included instruction.
    let tail: HashSet<usize> = seen.iter().copied().filter(|pc| *pc > back).collect();
    if !tail.is_empty() {
        fn returns_only_through_tail(
            func: &Function,
            pc: usize,
            tail: &HashSet<usize>,
            color: &mut HashMap<usize, u8>,
        ) -> bool {
            if func.instructions.get(pc).map(|i| i.opcode) == Some(Opcode::Ret) {
                return true;
            }
            match color.get(&pc).copied().unwrap_or(0) {
                1 => return false, // A cycle may not terminate; reject it.
                2 => return true,
                _ => {}
            }
            if !tail.contains(&pc) {
                return false;
            }
            color.insert(pc, 1);
            let next_pcs = successors(func, pc);
            if next_pcs.is_empty() || next_pcs.iter().any(|next| !tail.contains(next)) {
                return false;
            }
            for next in next_pcs {
                if !returns_only_through_tail(func, next, tail, color) {
                    return false;
                }
            }
            color.insert(pc, 2);
            true
        }
        let mut color = HashMap::<usize, u8>::new();
        for &pc in &tail {
            if !returns_only_through_tail(func, pc, &tail, &mut color) {
                return None;
            }
        }
    }

    Some(seen.into_iter().collect())
}

fn is_int_operand(op: &OperandType, state: &[ValType], args: &[ValType]) -> bool {
    operand_type_with_args(op, state, args) == ValType::Int
}

fn is_int_register_operand(op: Option<&OperandType>, state: &[ValType]) -> bool {
    match op {
        Some(OperandType::Register(r)) => state.get(*r).copied() == Some(ValType::Int),
        _ => false,
    }
}

fn typed_int_operand_compatible(
    op: &OperandType,
    state: &[ValType],
    argument_types: &[ValType],
    int_demand: &HashSet<usize>,
) -> bool {
    match operand_type_with_args(op, state, argument_types) {
        ValType::Int => true,
        // An explicitly typed I* opcode is itself a bytecode-level type
        // contract. Unknown facts are accepted only for registers that are
        // read in an integer-typed position (or copied to one); known conflicting
        // types are never overridden.
        ValType::Unknown => matches!(op, OperandType::Register(r) if int_demand.contains(r)),
        _ => false,
    }
}

fn predicated_integer_demand(
    func: &Function,
    region: &[usize],
    induction: usize,
    limit: &OperandType,
) -> HashSet<usize> {
    let mut demand = HashSet::from([induction]);
    if let OperandType::Register(r) = limit {
        demand.insert(*r);
    }

    // Explicit typed integer instructions establish integer requirements for
    // their value operands. Do not infer types from generic dynamic opcodes.
    for &pc in region {
        let inst = &func.instructions[pc];
        let integer_indices: &[usize] = match inst.opcode {
            Opcode::IAddRRR
            | Opcode::ISubRRR
            | Opcode::IMulRRR
            | Opcode::IEqRRR
            | Opcode::ILtRRR
            | Opcode::IGtRRR
            | Opcode::IGtJmpRR
            | Opcode::ILtJmpRR
            | Opcode::IEqJmpRR => &[0, 1],
            Opcode::IAddRCR
            | Opcode::ISubRCR
            | Opcode::IMulRCR
            | Opcode::IDivRCR
            | Opcode::IRemRCR
            | Opcode::IEqRCR
            | Opcode::ILtRCR
            | Opcode::ILtCRR
            | Opcode::IGtRCR
            | Opcode::IGtCRR
            | Opcode::IGtJmpRC
            | Opcode::ILtJmpRC
            | Opcode::IEqJmpRC => &[0],
            Opcode::ISubCRR => &[1],
            Opcode::IArrayGet => &[1],
            _ => &[],
        };
        for &idx in integer_indices {
            if let Some(OperandType::Register(r)) = inst.operands.get(idx) {
                demand.insert(*r);
            }
        }
    }

    // LoadConst with a register source is a copy in this VM. Propagate an
    // integer requirement backwards through copies so argument-derived loop
    // values can be recognized even when interprocedural inference is Unknown.
    loop {
        let mut changed = false;
        for &pc in region {
            let inst = &func.instructions[pc];
            if inst.opcode != Opcode::LoadConst {
                continue;
            }
            let Some(OperandType::Register(dst)) = inst.operands.get(1) else {
                continue;
            };
            let Some(OperandType::Register(src)) = inst.operands.first() else {
                continue;
            };
            if demand.contains(dst) && demand.insert(*src) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    demand
}

fn predicated_instruction_is_supported(
    inst: &Instruction,
    state: &[ValType],
    argument_types: &[ValType],
    int_demand: &HashSet<usize>,
    induction: usize,
) -> bool {
    let ops = &inst.operands;
    let int_binary = |i: usize| {
        ops.get(i)
            .map(|o| is_int_operand(o, state, argument_types))
            .unwrap_or(false)
    };
    let typed_int_binary = |i: usize| {
        ops.get(i)
            .map(|o| typed_int_operand_compatible(o, state, argument_types, int_demand))
            .unwrap_or(false)
    };

    match inst.opcode {
        Opcode::Nop | Opcode::Jump => true,
        Opcode::LoadConst => {
            if is_register(ops.get(1)).is_none() { return false; }
            let Some(source) = ops.first() else { return false };
            match source {
                OperandType::Constant(Value::Int(_) | Value::Bool(_)) => true,
                OperandType::Register(r) => {
                    let ty = state.get(*r).copied().unwrap_or(ValType::Unknown);
                    matches!(ty, ValType::Int | ValType::Bool)
                        || (ty == ValType::Unknown && int_demand.contains(r))
                }
                OperandType::Argument(a) => matches!(argument_types.get(*a), Some(ValType::Int | ValType::Bool)),
                _ => false,
            }
        }
        Opcode::IArrayGet => {
            if ops.len() < 3 || is_register(ops.get(2)).is_none() || !typed_int_binary(1) {
                return false;
            }
            let array_ty = ops.first()
                .map(|o| operand_type_with_args(o, state, argument_types))
                .unwrap_or(ValType::Unknown);
            array_ty == ValType::IntArray
                && matches!(ops.first(), Some(OperandType::Register(_) | OperandType::Argument(_)))
        }
        // Generic dynamic operations are only vectorized when the analysis has
        // proven their operands are integers. A specialized I* opcode is a type
        // contract and may be used where flow analysis has an Unknown fact.
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Eq | Opcode::Lt | Opcode::Gt => {
            ops.len() >= 3 && is_register(ops.get(2)).is_some() && int_binary(0) && int_binary(1)
        }
        Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR
        | Opcode::IEqRRR | Opcode::ILtRRR | Opcode::IGtRRR => {
            ops.len() >= 3 && is_register(ops.get(2)).is_some()
                && typed_int_binary(0) && typed_int_binary(1)
        }
        Opcode::IAddRCR | Opcode::ISubRCR | Opcode::IMulRCR
        | Opcode::IEqRCR | Opcode::ILtRCR | Opcode::IGtRCR => {
            ops.len() >= 3 && is_register(ops.first()).is_some()
                && matches!(ops.get(1), Some(OperandType::Constant(Value::Int(_))))
                && is_register(ops.get(2)).is_some() && typed_int_binary(0)
        }
        Opcode::ISubCRR | Opcode::ILtCRR | Opcode::IGtCRR => {
            ops.len() >= 3 && matches!(ops.first(), Some(OperandType::Constant(Value::Int(_))))
                && is_register(ops.get(1)).is_some() && is_register(ops.get(2)).is_some()
                && typed_int_binary(1)
        }
        Opcode::IDivRCR | Opcode::IRemRCR => {
            ops.len() >= 3
                && is_register(ops.first()).is_some()
                && matches!(ops.get(1), Some(OperandType::Constant(Value::Int(divisor))) if *divisor != 0 && *divisor != -1)
                && is_register(ops.get(2)).is_some()
                && typed_int_binary(0)
        }
        Opcode::JumpTrue | Opcode::JumpFalse | Opcode::IJumpTrueR | Opcode::IJumpFalseR => {
            matches!(ops.first(), Some(OperandType::Register(r)) if state.get(*r).copied() == Some(ValType::Bool))
                && matches!(ops.get(1), Some(OperandType::Offset(_)))
        }
        Opcode::IGtJmpRR | Opcode::ILtJmpRR | Opcode::IEqJmpRR => {
            ops.len() >= 3 && typed_int_binary(0) && typed_int_binary(1)
                && matches!(ops.get(2), Some(OperandType::Offset(_)))
        }
        Opcode::IGtJmpRC | Opcode::ILtJmpRC | Opcode::IEqJmpRC => {
            ops.len() >= 3 && typed_int_binary(0) && typed_int_binary(1)
                && matches!(ops.get(2), Some(OperandType::Offset(_)))
        }
        Opcode::Ret => ops.first().map(|o| {
            let ty = operand_type_with_args(o, state, argument_types);
            ty == ValType::Int
                || matches!(o, OperandType::Register(r) if *r == induction || (ty == ValType::Unknown && int_demand.contains(r)))
        }).unwrap_or(false),
        _ => false,
    }
}

fn predicated_vector_opcode(op: Opcode) -> Option<Opcode> {
    Some(match op {
        Opcode::Add | Opcode::IAddRRR | Opcode::IAddRCR => Opcode::VAddI,
        Opcode::Sub | Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR => Opcode::VSubI,
        Opcode::Mul | Opcode::IMulRRR | Opcode::IMulRCR => Opcode::VMulI,
        Opcode::IDivRCR => Opcode::VDivI,
        Opcode::IRemRCR => Opcode::VRemI,
        Opcode::Eq | Opcode::IEqRRR | Opcode::IEqRCR => Opcode::VCmpEqI,
        Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR | Opcode::ILtCRR => Opcode::VCmpLtI,
        Opcode::Gt | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::IGtCRR => Opcode::VCmpGtI,
        _ => return None,
    })
}

fn predicated_vector_operand(
    op: &OperandType,
    induction: usize,
    vector_induction: usize,
    vector_registers: &HashMap<usize, usize>,
    argument_types: &[ValType],
    next_reg: &mut usize,
    emitted: &mut Vec<Instruction>,
    constant_vectors: &mut HashMap<i64, usize>,
    argument_vectors: &mut HashMap<usize, usize>,
) -> Option<OperandType> {
    match op {
        OperandType::Register(r) if *r == induction => {
            Some(OperandType::Register(vector_induction))
        }
        OperandType::Register(r) => vector_registers.get(r).copied().map(OperandType::Register),
        OperandType::Constant(Value::Int(v)) => {
            let reg = if let Some(reg) = constant_vectors.get(v) {
                *reg
            } else {
                let reg = alloc_temp(next_reg);
                emitted.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        OperandType::Constant(Value::Int(*v)),
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(reg),
                    ],
                ));
                constant_vectors.insert(*v, reg);
                reg
            };
            Some(OperandType::Register(reg))
        }
        OperandType::Constant(Value::Bool(v)) => {
            let scalar = if *v { -1 } else { 0 };
            let reg = if let Some(reg) = constant_vectors.get(&scalar) {
                *reg
            } else {
                let reg = alloc_temp(next_reg);
                emitted.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        OperandType::Constant(Value::Int(scalar)),
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(reg),
                    ],
                ));
                constant_vectors.insert(scalar, reg);
                reg
            };
            Some(OperandType::Register(reg))
        }
        OperandType::Argument(a) if argument_types.get(*a).copied() == Some(ValType::Int) => {
            let reg = if let Some(reg) = argument_vectors.get(a) {
                *reg
            } else {
                let reg = alloc_temp(next_reg);
                emitted.push(Instruction::new(
                    Opcode::VSplatI,
                    vec![
                        OperandType::Argument(*a),
                        OperandType::Constant(Value::Int(SIMD_WIDTH)),
                        OperandType::Register(reg),
                    ],
                ));
                argument_vectors.insert(*a, reg);
                reg
            };
            Some(OperandType::Register(reg))
        }
        _ => None,
    }
}

fn predicated_emit_pc_update(
    out: &mut Vec<Instruction>,
    exec_mask: usize,
    target_vec: usize,
    vector_pc: usize,
) {
    out.push(Instruction::new(
        Opcode::VSelectI,
        vec![
            OperandType::Register(exec_mask),
            OperandType::Register(target_vec),
            OperandType::Register(vector_pc),
            OperandType::Register(vector_pc),
        ],
    ));
}

#[derive(Clone)]
enum ArrayLengthRequirement {
    AtLeastConstant(i64),
    AtLeastOperand(OperandType),
}

fn required_array_length_for_index(
    func: &Function,
    region: &[usize],
    induction: usize,
    limit: &OperandType,
    index: &OperandType,
) -> Option<ArrayLengthRequirement> {
    match index {
        OperandType::Register(r) if *r == induction => {
            return Some(ArrayLengthRequirement::AtLeastOperand(limit.clone()));
        }
        OperandType::Constant(Value::Int(value)) => {
            // Scalar typed-array access clamps a negative index to zero.
            return Some(ArrayLengthRequirement::AtLeastConstant(
                (*value).max(0).checked_add(1)?,
            ));
        }
        OperandType::Register(index_reg) => {
            let defs: Vec<(usize, &Instruction)> = region
                .iter()
                .filter_map(|pc| {
                    let inst = &func.instructions[*pc];
                    instruction_written_registers(inst)
                        .contains(index_reg)
                        .then_some((*pc, inst))
                })
                .collect();
            if defs.len() != 1 {
                return None;
            }
            let (def_pc, def) = defs[0];
            if def.opcode != Opcode::IRemRCR
                || is_register(def.operands.first()).is_none()
                || is_register(def.operands.get(2)) != Some(*index_reg)
            {
                return None;
            }
            let divisor = constant_int(def.operands.get(1)?)?;
            if divisor <= 0 {
                return None;
            }
            let dividend = is_register(def.operands.first())?;

            // Prove that this modulo executes only on the nonnegative path:
            // `dividend > 0`, immediately followed by a false-branch over the
            // modulo. This covers digit loops and avoids assuming Rust `%`
            // bounds for a negative dividend.
            if def_pc < 2 {
                return None;
            }
            let branch_pc = def_pc - 1;
            let compare_pc = def_pc - 2;
            let branch = &func.instructions[branch_pc];
            let compare = &func.instructions[compare_pc];
            if !matches!(branch.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR)
                || !matches!(compare.opcode, Opcode::IGtRCR)
                || is_register(branch.operands.first()) != is_register(compare.operands.get(2))
                || is_register(compare.operands.first()) != Some(dividend)
                || compare.operands.get(1).and_then(constant_int) != Some(0)
            {
                return None;
            }
            let offset = match branch.operands.get(1)? {
                OperandType::Offset(offset) => *offset,
                _ => return None,
            };
            if target_from_offset(branch_pc, offset, func.instructions.len())? <= def_pc {
                return None;
            }
            // The local guard is only a proof if no other edge can enter the
            // modulo instruction and bypass `dividend > 0`. Allow the normal
            // fallthrough from its immediately preceding JumpFalse, but reject
            // every other predecessor (including unreachable-looking external
            // branches; conservatism is preferable to an invalid gather proof).
            for source_pc in 0..func.instructions.len() {
                if source_pc != branch_pc && successors(func, source_pc).contains(&def_pc) {
                    return None;
                }
            }
            return Some(ArrayLengthRequirement::AtLeastConstant(divisor));
        }
        _ => None,
    }
}

fn build_predicated_simd_prefix(
    func: &Function,
    head: usize,
    back: usize,
    types: &TypeAnalysis,
    argument_types: &[ValType],
    safe_readonly_array_args: &HashSet<usize>,
) -> Option<(Vec<Instruction>, usize)> {
    // In debug builds scalar typed arithmetic can panic on overflow while the
    // vector helpers deliberately wrap. Keep the original loop until those
    // semantics are unified across interpreter and SIMD implementations.
    if cfg!(debug_assertions) {
        return None;
    }
    let instrs = &func.instructions;
    let header = instrs.get(head)?;
    let branch = instrs.get(head + 1)?;
    let latch = back.checked_sub(1)?;
    if !matches!(header.opcode, Opcode::ILtRRR | Opcode::ILtRCR)
        || header.operands.len() < 3
        || !matches!(branch.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR)
        || !matches!(branch.operands.first(), Some(OperandType::Register(_)))
        || !matches!(branch.operands.get(1), Some(OperandType::Offset(_)))
        || !matches!(instrs.get(back)?.opcode, Opcode::Jump)
    {
        return None;
    }

    let induction = is_register(header.operands.first())?;
    let condition = is_register(header.operands.get(2))?;
    if is_register(branch.operands.first())? != condition {
        return None;
    }
    let limit = header.operands.get(1)?.clone();
    if !matches!(header.opcode, Opcode::ILtRRR)
        && !matches!(&limit, OperandType::Constant(Value::Int(_)))
    {
        return None;
    }
    if matches!(&limit, OperandType::Constant(Value::Int(v)) if *v < 0) {
        return None;
    }
    let head_state = types.before.get(head)?.as_ref()?;
    let induction_ty = head_state
        .get(induction)
        .copied()
        .unwrap_or(ValType::Unknown);
    let limit_ty = operand_type_with_args(&limit, head_state, argument_types);
    // ILtRRR / ILtRCR are already-specialized integer instructions. Unknown
    // here means our analysis could not recover an argument's type, not that a
    // generic comparison is being assumed to be integer arithmetic.
    if !matches!(induction_ty, ValType::Int | ValType::Unknown)
        || !matches!(limit_ty, ValType::Int | ValType::Unknown)
    {
        return None;
    }

    // Only unit-step, ascending induction loops qualify. The unmodified loop is
    // retained after this prefix and handles tails and every failed guard.
    let inc = instrs.get(latch)?;
    if !matches!(inc.opcode, Opcode::IAddRCR)
        || is_register(inc.operands.first())? != induction
        || constant_int(inc.operands.get(1)?)? != 1
        || is_register(inc.operands.get(2))? != induction
    {
        return None;
    }
    if target_from_offset(
        back,
        match instrs[back].operands.first()? {
            OperandType::Offset(off) => *off,
            _ => return None,
        },
        instrs.len(),
    )? != head
    {
        return None;
    }
    let exit_target = target_from_offset(
        head + 1,
        match branch.operands.get(1)? {
            OperandType::Offset(off) => *off,
            _ => return None,
        },
        instrs.len(),
    )?;

    let region = predicated_loop_region(func, head, back)?;
    let region_set: HashSet<usize> = region.iter().copied().collect();
    if region_set.contains(&exit_target) {
        return None;
    }
    // Reaching the latch by ordinary fallthrough means this candidate is done.
    // An explicit branch to the latch/back-edge has different scalar semantics:
    // it executes the increment or jumps to the header. The predicated dispatcher
    // intentionally does not model those alternate loop-control edges yet.
    for &pc in &region {
        let inst = &instrs[pc];
        if let Some(offset_idx) = jump_offset_index(inst) {
            if let Some(OperandType::Offset(off)) = inst.operands.get(offset_idx) {
                let target = target_from_offset(pc, *off, instrs.len())?;
                if target == latch || target == back {
                    return None;
                }
            }
        }
    }
    let int_demand = predicated_integer_demand(func, &region, induction, &limit);

    let mut array_base_ops = Vec::<OperandType>::new();
    let mut array_length_requirements = Vec::<(OperandType, ArrayLengthRequirement)>::new();
    let mut array_base_regs = HashSet::<usize>::new();
    let mut has_array_read = false;
    let mut reads: HashMap<usize, HashSet<usize>> = HashMap::new();
    let mut writes: HashMap<usize, HashSet<usize>> = HashMap::new();
    let mut all_regs = BTreeSet::<usize>::new();

    for &pc in &region {
        if pc == latch {
            continue;
        }
        let inst = &instrs[pc];
        let state = types.before.get(pc)?.as_ref()?;
        if !predicated_instruction_is_supported(inst, state, argument_types, &int_demand, induction)
        {
            return None;
        }
        if inst.opcode == Opcode::IArrayGet {
            has_array_read = true;
            let arr = inst.operands.first()?.clone();
            let index = inst.operands.get(1)?;
            let requirement =
                required_array_length_for_index(func, &region, induction, &limit, index)?;
            array_length_requirements.push((arr.clone(), requirement));
            if !array_base_ops.contains(&arr) {
                array_base_ops.push(arr.clone());
            }
            if let OperandType::Register(r) = arr {
                array_base_regs.insert(r);
            }
        }
        let r: HashSet<usize> = instruction_reads(inst).into_iter().collect();
        let w: HashSet<usize> = instruction_written_registers(inst).into_iter().collect();
        for reg in r.iter().chain(w.iter()) {
            all_regs.insert(*reg);
        }
        reads.insert(pc, r);
        writes.insert(pc, w);

        // The induction value belongs to the batch, not the scalar loop body.
        // A body write to it would invalidate the lane-to-candidate mapping.
        if instruction_written_registers(inst).contains(&induction) {
            return None;
        }
    }
    if !has_array_read {
        return None;
    }
    // Distinct arrays may be read, but every base must be a proven IntArray.
    // `array_base_ops` stays scalar; all numeric/index registers are lane-mapped.
    for arr in &array_base_ops {
        let state = types.before.get(head + 2)?.as_ref()?;
        if operand_type_with_args(arr, state, argument_types) != ValType::IntArray {
            return None;
        }
        let readonly_proven = match arr {
            OperandType::Argument(arg) => safe_readonly_array_args.contains(arg),
            OperandType::Register(reg) => {
                is_private_array_register(func, *reg, head, back)
                    || array_argument_origin_before(func, *reg, head)
                        .map(|arg| safe_readonly_array_args.contains(&arg))
                        .unwrap_or(false)
            }
            _ => false,
        };
        if !readonly_proven {
            return None;
        }
    }

    // Only writes on the repeating candidate path can be loop-carried or leak
    // into the scalar continuation. Writes in the post-backedge return-only
    // epilogue are lane-local: those paths terminate in Ret and cannot reach the
    // next candidate or the normal loop-exhaustion return. Include their
    // registers in `all_regs`/liveness, but do not mistake them for loop writes.
    let mut loop_writes = HashSet::<usize>::new();
    for (&pc, ws) in &writes {
        if pc == latch || pc > back {
            continue;
        }
        loop_writes.extend(ws.iter().copied());
    }
    for reg in &loop_writes {
        if *reg == induction {
            return None;
        }
        for pc in (head + 1)..instrs.len() {
            if region_set.contains(&pc) {
                continue;
            }
            if instruction_reads(&instrs[pc]).contains(reg) {
                return None;
            }
        }
    }
    // The loop limit must be invariant across every candidate iteration.
    if let OperandType::Register(limit_reg) = &limit {
        if loop_writes.contains(limit_reg) {
            return None;
        }
    }

    // Compute standard CFG liveness over the candidate body. Every live-in is
    // broadcast from the scalar register at the start of each four-item batch.
    let mut live_in: HashMap<usize, HashSet<usize>> =
        region.iter().map(|pc| (*pc, HashSet::new())).collect();
    let mut live_out: HashMap<usize, HashSet<usize>> =
        region.iter().map(|pc| (*pc, HashSet::new())).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for &pc in region.iter().rev() {
            let mut out = HashSet::new();
            if pc != latch && instrs[pc].opcode != Opcode::Ret {
                for succ in successors(func, pc) {
                    if region_set.contains(&succ) {
                        if let Some(s) = live_in.get(&succ) {
                            out.extend(s.iter().copied());
                        }
                    }
                }
            }
            let mut input = reads.get(&pc).cloned().unwrap_or_default();
            let defs = writes.get(&pc).cloned().unwrap_or_default();
            input.extend(out.difference(&defs).copied());
            if live_out.get(&pc) != Some(&out) {
                live_out.insert(pc, out);
                changed = true;
            }
            if live_in.get(&pc) != Some(&input) {
                live_in.insert(pc, input);
                changed = true;
            }
        }
    }
    let live_ins = live_in.get(&(head + 2))?;
    for r in live_ins {
        if *r == induction || array_base_regs.contains(r) {
            continue;
        }
        if loop_writes.contains(r) {
            return None;
        }
        let ty = types
            .before
            .get(head + 2)?
            .as_ref()?
            .get(*r)
            .copied()
            .unwrap_or(ValType::Unknown);
        if ty != ValType::Int && !(ty == ValType::Unknown && int_demand.contains(r)) {
            // Bool live-ins are Values::Bool, but VSplatI accepts integer lanes.
            // Unknown is accepted only when a typed integer use proves the
            // operand contract; known conflicting types remain a hard bailout.
            return None;
        }
    }
    // Every operand register must be represented by a lane register, except for
    // array base registers which remain scalar shared references.
    for r in &all_regs {
        if *r != induction && !array_base_regs.contains(r) {
            let state = types.before.get(head + 2)?.as_ref()?;
            if !matches!(
                state.get(*r).copied().unwrap_or(ValType::Unknown),
                ValType::Int | ValType::Bool | ValType::Unknown
            ) {
                return None;
            }
        }
    }

    let mut next_reg = register_count_needed(func);
    let vector_induction = alloc_temp(&mut next_reg);
    let vector_pc = alloc_temp(&mut next_reg);
    let vector_retmask = alloc_temp(&mut next_reg);
    let vector_retvalue = alloc_temp(&mut next_reg);
    let vector_exec = alloc_temp(&mut next_reg);
    let vector_pc_const = alloc_temp(&mut next_reg);
    let vector_done_mask = alloc_temp(&mut next_reg);
    let scalar_sum = alloc_temp(&mut next_reg);
    let scalar_count = alloc_temp(&mut next_reg);
    let scalar_mask = alloc_temp(&mut next_reg);
    let scalar_retvalue = alloc_temp(&mut next_reg);
    let scalar_condition = alloc_temp(&mut next_reg);
    let remaining = alloc_temp(&mut next_reg);
    let vector_lane_ids = alloc_temp(&mut next_reg);
    let vector_lane_bound_1 = alloc_temp(&mut next_reg);
    let vector_lane_bound_2 = alloc_temp(&mut next_reg);
    let vector_lane_mask_1 = alloc_temp(&mut next_reg);
    let vector_lane_mask_2 = alloc_temp(&mut next_reg);

    let mut vector_registers = HashMap::<usize, usize>::new();
    vector_registers.insert(induction, vector_induction);
    for r in &all_regs {
        if *r == induction || array_base_regs.contains(r) {
            continue;
        }
        vector_registers.insert(*r, alloc_temp(&mut next_reg));
    }

    let mut prefix = Vec::<Instruction>::new();
    let mut const_vectors = HashMap::<i64, usize>::new();
    let mut arg_vectors = HashMap::<usize, usize>::new();
    let v_zero = {
        let r = alloc_temp(&mut next_reg);
        prefix.push(Instruction::new(
            Opcode::VSplatI,
            vec![
                OperandType::Constant(Value::Int(0)),
                OperandType::Constant(Value::Int(SIMD_WIDTH)),
                OperandType::Register(r),
            ],
        ));
        const_vectors.insert(0, r);
        r
    };
    let v_minus_one = {
        let r = alloc_temp(&mut next_reg);
        prefix.push(Instruction::new(
            Opcode::VSplatI,
            vec![
                OperandType::Constant(Value::Int(-1)),
                OperandType::Constant(Value::Int(SIMD_WIDTH)),
                OperandType::Register(r),
            ],
        ));
        const_vectors.insert(-1, r);
        r
    };
    let v_done = v_minus_one;
    prefix.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Constant(Value::Int(0)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_lane_ids),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSplatI,
        vec![
            OperandType::Constant(Value::Int(1)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_lane_bound_1),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSplatI,
        vec![
            OperandType::Constant(Value::Int(2)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_lane_bound_2),
        ],
    ));

    // Re-check guards at every batch. If fewer than four candidates remain,
    // fall through to the original scalar loop at its current induction value.
    let mut fallback_branches = Vec::<usize>::new();
    // Gather executes all four physical lanes, including lanes whose virtual PC
    // is elsewhere. Guard every lookup table's required extent up front so even
    // inactive lanes (initially zero, thereafter holding a previously valid
    // index) cannot issue an out-of-bounds gather. If a bound cannot be proved,
    // required_array_length_for_index() has already rejected the transform.
    for (array, requirement) in &array_length_requirements {
        prefix.push(Instruction::new(
            Opcode::ArrayLen,
            vec![array.clone(), OperandType::Register(remaining)],
        ));
        let cmp = match requirement {
            ArrayLengthRequirement::AtLeastConstant(minimum) => Instruction::new(
                Opcode::ILtRCR,
                vec![
                    OperandType::Register(remaining),
                    OperandType::Constant(Value::Int(*minimum)),
                    OperandType::Register(scalar_condition),
                ],
            ),
            ArrayLengthRequirement::AtLeastOperand(bound) => match bound {
                OperandType::Register(_) => Instruction::new(
                    Opcode::ILtRRR,
                    vec![
                        OperandType::Register(remaining),
                        bound.clone(),
                        OperandType::Register(scalar_condition),
                    ],
                ),
                OperandType::Constant(Value::Int(_)) => Instruction::new(
                    Opcode::ILtRCR,
                    vec![
                        OperandType::Register(remaining),
                        bound.clone(),
                        OperandType::Register(scalar_condition),
                    ],
                ),
                _ => return None,
            },
        };
        prefix.push(cmp);
        let branch_idx = prefix.len();
        prefix.push(Instruction::new(
            Opcode::IJumpTrueR,
            vec![
                OperandType::Register(scalar_condition),
                OperandType::Offset(0),
            ],
        ));
        fallback_branches.push(branch_idx);
    }

    let batch_head = prefix.len();
    prefix.push(Instruction::new(
        Opcode::IGtRCR,
        vec![
            OperandType::Register(induction),
            OperandType::Constant(Value::Int(-1)),
            OperandType::Register(scalar_condition),
        ],
    ));
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![
            OperandType::Register(scalar_condition),
            OperandType::Offset(0),
        ],
    ));
    let mut guarded_header = header.clone();
    let header_dst_idx = guarded_header.operands.len().checked_sub(1)?;
    guarded_header.operands[header_dst_idx] = OperandType::Register(scalar_condition);
    prefix.push(guarded_header);
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![
            OperandType::Register(scalar_condition),
            OperandType::Offset(0),
        ],
    ));
    match &limit {
        OperandType::Register(limit_reg) => prefix.push(Instruction::new(
            Opcode::ISubRRR,
            vec![
                OperandType::Register(*limit_reg),
                OperandType::Register(induction),
                OperandType::Register(remaining),
            ],
        )),
        OperandType::Constant(Value::Int(v)) => prefix.push(Instruction::new(
            Opcode::ISubCRR,
            vec![
                OperandType::Constant(Value::Int(*v)),
                OperandType::Register(induction),
                OperandType::Register(remaining),
            ],
        )),
        _ => return None,
    }
    prefix.push(Instruction::new(
        Opcode::IGtRCR,
        vec![
            OperandType::Register(remaining),
            OperandType::Constant(Value::Int(SIMD_WIDTH - 1)),
            OperandType::Register(scalar_condition),
        ],
    ));
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(
        Opcode::IJumpFalseR,
        vec![
            OperandType::Register(scalar_condition),
            OperandType::Offset(0),
        ],
    ));

    prefix.push(Instruction::new(
        Opcode::VIotaI,
        vec![
            OperandType::Register(induction),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_induction),
        ],
    ));
    // Initialize every lane register on every batch. Liveness ensures every
    // read-before-write is a proved Int live-in and cannot be modified in-body.
    // Emit initialization in register-number order so optimized bytecode is
    // deterministic across runs despite the lookup map's randomized iteration.
    for scalar_reg in &all_regs {
        if *scalar_reg == induction || array_base_regs.contains(scalar_reg) {
            continue;
        }
        let vector_reg = *vector_registers.get(scalar_reg)?;
        if live_ins.contains(scalar_reg) {
            prefix.push(Instruction::new(
                Opcode::VSplatI,
                vec![
                    OperandType::Register(*scalar_reg),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(vector_reg),
                ],
            ));
        } else {
            prefix.push(Instruction::new(
                Opcode::VSplatI,
                vec![
                    OperandType::Constant(Value::Int(0)),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(vector_reg),
                ],
            ));
        }
    }
    prefix.push(Instruction::new(
        Opcode::VSplatI,
        vec![
            OperandType::Constant(Value::Int(head as i64 + 2)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_pc),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSplatI,
        vec![
            OperandType::Constant(Value::Int(0)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_retmask),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSplatI,
        vec![
            OperandType::Constant(Value::Int(0)),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(vector_retvalue),
        ],
    ));

    let dispatch_head = prefix.len();
    // Leave dispatch only when every lane has reached a return or the outer
    // loop latch. -1 is impossible as a real bytecode PC.
    prefix.push(Instruction::new(
        Opcode::VCmpEqI,
        vec![
            OperandType::Register(vector_pc),
            OperandType::Register(v_done),
            OperandType::Register(vector_done_mask),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSumI,
        vec![
            OperandType::Register(vector_done_mask),
            OperandType::Register(scalar_sum),
        ],
    ));
    let all_done_branch = prefix.len();
    prefix.push(Instruction::new(
        Opcode::IEqJmpRC,
        vec![
            OperandType::Register(scalar_sum),
            OperandType::Constant(Value::Int(-4)),
            OperandType::Offset(0),
        ],
    ));

    let mut skip_branches = Vec::<usize>::new();
    let mut immediate_return_branches = Vec::<usize>::new();
    let mut prune_lane_1_branches = Vec::<usize>::new();
    let mut prune_lane_2_branches = Vec::<usize>::new();

    for &pc in &region {
        let inst = &instrs[pc];
        let skip_branch = {
            let pc_as_i64 = i64::try_from(pc).ok()?;
            prefix.push(Instruction::new(
                Opcode::VSplatI,
                vec![
                    OperandType::Constant(Value::Int(pc_as_i64)),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(vector_pc_const),
                ],
            ));
            prefix.push(Instruction::new(
                Opcode::VCmpEqI,
                vec![
                    OperandType::Register(vector_pc),
                    OperandType::Register(vector_pc_const),
                    OperandType::Register(vector_exec),
                ],
            ));
            prefix.push(Instruction::new(
                Opcode::VSumI,
                vec![
                    OperandType::Register(vector_exec),
                    OperandType::Register(scalar_count),
                ],
            ));
            let branch_idx = prefix.len();
            prefix.push(Instruction::new(
                Opcode::IEqJmpRC,
                vec![
                    OperandType::Register(scalar_count),
                    OperandType::Constant(Value::Int(0)),
                    OperandType::Offset(0),
                ],
            ));
            branch_idx
        };
        if pc == latch {
            predicated_emit_pc_update(&mut prefix, vector_exec, v_done, vector_pc);
        } else {
            match inst.opcode {
                Opcode::Nop => {
                    let next = pc.checked_add(1)?;
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int(next as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(
                        &mut prefix,
                        vector_exec,
                        is_register(Some(&next_vec))?,
                        vector_pc,
                    );
                }
                Opcode::LoadConst => {
                    let dst = is_register(inst.operands.get(1))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let source = predicated_vector_operand(
                        inst.operands.first()?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(vector_exec),
                            source,
                            OperandType::Register(vector_dst),
                            OperandType::Register(vector_dst),
                        ],
                    ));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(
                        &mut prefix,
                        vector_exec,
                        is_register(Some(&next_vec))?,
                        vector_pc,
                    );
                }
                Opcode::IArrayGet => {
                    let dst = is_register(inst.operands.get(2))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let index = predicated_vector_operand(
                        inst.operands.get(1)?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let index_reg = is_register(Some(&index))?;
                    // Scalar IArrayGet clamps negative indices to zero; VGatherI
                    // does not. Normalize indices to preserve that behavior.
                    let nonnegative = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        Opcode::VCmpGtI,
                        vec![
                            OperandType::Register(index_reg),
                            OperandType::Register(v_minus_one),
                            OperandType::Register(nonnegative),
                        ],
                    ));
                    let safe_index = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(nonnegative),
                            OperandType::Register(index_reg),
                            OperandType::Register(v_zero),
                            OperandType::Register(safe_index),
                        ],
                    ));
                    let gathered = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        Opcode::VGatherI,
                        vec![
                            inst.operands[0].clone(),
                            OperandType::Register(safe_index),
                            OperandType::Register(gathered),
                        ],
                    ));
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(vector_exec),
                            OperandType::Register(gathered),
                            OperandType::Register(vector_dst),
                            OperandType::Register(vector_dst),
                        ],
                    ));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(
                        &mut prefix,
                        vector_exec,
                        is_register(Some(&next_vec))?,
                        vector_pc,
                    );
                }
                Opcode::Jump => {
                    let offset = match inst.operands.first()? {
                        OperandType::Offset(o) => *o,
                        _ => return None,
                    };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    let target_vec = if target == back {
                        v_done
                    } else {
                        if !region_set.contains(&target) {
                            return None;
                        }
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)),
                            induction,
                            vector_induction,
                            &vector_registers,
                            argument_types,
                            &mut next_reg,
                            &mut prefix,
                            &mut const_vectors,
                            &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    predicated_emit_pc_update(&mut prefix, vector_exec, target_vec, vector_pc);
                }
                Opcode::JumpTrue | Opcode::JumpFalse | Opcode::IJumpTrueR | Opcode::IJumpFalseR => {
                    let offset = match inst.operands.get(1)? {
                        OperandType::Offset(o) => *o,
                        _ => return None,
                    };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    if target != back && !region_set.contains(&target) {
                        return None;
                    }
                    let condition_op = inst.operands.first()?;
                    let condition_vec = predicated_vector_operand(
                        condition_op,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let condition_reg = is_register(Some(&condition_vec))?;
                    let condition_mask =
                        if matches!(inst.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR) {
                            let mask = alloc_temp(&mut next_reg);
                            prefix.push(Instruction::new(
                                Opcode::VCmpEqI,
                                vec![
                                    OperandType::Register(condition_reg),
                                    OperandType::Register(v_zero),
                                    OperandType::Register(mask),
                                ],
                            ));
                            mask
                        } else {
                            condition_reg
                        };
                    let target_vec = if target == back {
                        v_done
                    } else {
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)),
                            induction,
                            vector_induction,
                            &vector_registers,
                            argument_types,
                            &mut next_reg,
                            &mut prefix,
                            &mut const_vectors,
                            &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    let fallthrough_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let selected_pc = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(condition_mask),
                            OperandType::Register(target_vec),
                            fallthrough_vec,
                            OperandType::Register(selected_pc),
                        ],
                    ));
                    predicated_emit_pc_update(&mut prefix, vector_exec, selected_pc, vector_pc);
                }
                Opcode::IGtJmpRR
                | Opcode::IGtJmpRC
                | Opcode::ILtJmpRR
                | Opcode::ILtJmpRC
                | Opcode::IEqJmpRR
                | Opcode::IEqJmpRC => {
                    let offset_idx = jump_offset_index(inst)?;
                    let offset = match inst.operands.get(offset_idx)? {
                        OperandType::Offset(o) => *o,
                        _ => return None,
                    };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    if target != back && !region_set.contains(&target) {
                        return None;
                    }
                    let cmp_op = match inst.opcode {
                        Opcode::IGtJmpRR | Opcode::IGtJmpRC => Opcode::VCmpGtI,
                        Opcode::ILtJmpRR | Opcode::ILtJmpRC => Opcode::VCmpLtI,
                        Opcode::IEqJmpRR | Opcode::IEqJmpRC => Opcode::VCmpEqI,
                        _ => unreachable!(),
                    };
                    let a = predicated_vector_operand(
                        inst.operands.first()?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let b = predicated_vector_operand(
                        inst.operands.get(1)?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let mask = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        cmp_op,
                        vec![a, b, OperandType::Register(mask)],
                    ));
                    let target_vec = if target == back {
                        v_done
                    } else {
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)),
                            induction,
                            vector_induction,
                            &vector_registers,
                            argument_types,
                            &mut next_reg,
                            &mut prefix,
                            &mut const_vectors,
                            &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    let fallthrough_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let selected_pc = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(mask),
                            OperandType::Register(target_vec),
                            fallthrough_vec,
                            OperandType::Register(selected_pc),
                        ],
                    ));
                    predicated_emit_pc_update(&mut prefix, vector_exec, selected_pc, vector_pc);
                }
                Opcode::Ret => {
                    let value = predicated_vector_operand(
                        inst.operands.first()?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(vector_exec),
                            value,
                            OperandType::Register(vector_retvalue),
                            OperandType::Register(vector_retvalue),
                        ],
                    ));
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(vector_exec),
                            OperandType::Register(v_minus_one),
                            OperandType::Register(vector_retmask),
                            OperandType::Register(vector_retmask),
                        ],
                    ));
                    predicated_emit_pc_update(&mut prefix, vector_exec, v_done, vector_pc);

                    // Preserve scalar iteration ordering: lane 0 returning wins
                    // immediately. If lane 1 or 2 returns first, freeze only
                    // higher lanes while lower lanes finish; lane 3 can return
                    // only after all lower lanes have completed.
                    for lane in 0..3usize {
                        prefix.push(Instruction::new(
                            Opcode::VExtractI,
                            vec![
                                OperandType::Register(vector_retmask),
                                OperandType::Constant(Value::Int(lane as i64)),
                                OperandType::Register(scalar_mask),
                            ],
                        ));
                        let jump_idx = prefix.len();
                        prefix.push(Instruction::new(
                            Opcode::IEqJmpRC,
                            vec![
                                OperandType::Register(scalar_mask),
                                OperandType::Constant(Value::Int(-1)),
                                OperandType::Offset(0),
                            ],
                        ));
                        if lane == 0 {
                            immediate_return_branches.push(jump_idx);
                        } else if lane == 1 {
                            prune_lane_1_branches.push(jump_idx);
                        } else {
                            prune_lane_2_branches.push(jump_idx);
                        }
                    }
                }
                _ => {
                    let vector_op = predicated_vector_opcode(inst.opcode)?;
                    let a = predicated_vector_operand(
                        inst.operands.first()?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let b = predicated_vector_operand(
                        inst.operands.get(1)?,
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    let dst = is_register(inst.operands.get(2))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let result = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(
                        vector_op,
                        vec![a, b, OperandType::Register(result)],
                    ));
                    prefix.push(Instruction::new(
                        Opcode::VSelectI,
                        vec![
                            OperandType::Register(vector_exec),
                            OperandType::Register(result),
                            OperandType::Register(vector_dst),
                            OperandType::Register(vector_dst),
                        ],
                    ));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)),
                        induction,
                        vector_induction,
                        &vector_registers,
                        argument_types,
                        &mut next_reg,
                        &mut prefix,
                        &mut const_vectors,
                        &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(
                        &mut prefix,
                        vector_exec,
                        is_register(Some(&next_vec))?,
                        vector_pc,
                    );
                }
            }
        }
        skip_branches.push(skip_branch);
        let after_case = prefix.len();
        prefix[skip_branch].operands[2] =
            OperandType::Offset((after_case as i64) - (skip_branch as i64));
    }

    // After a Ret case, prune labels jump back here without falling through the
    // rest of the static instruction schedule.
    let dispatch_back = prefix.len();
    prefix.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(
            dispatch_head as i64 - dispatch_back as i64,
        )],
    ));

    let prune_lane_1 = prefix.len();
    prefix.push(Instruction::new(
        Opcode::VCmpGtI,
        vec![
            OperandType::Register(vector_lane_ids),
            OperandType::Register(vector_lane_bound_1),
            OperandType::Register(vector_lane_mask_1),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSelectI,
        vec![
            OperandType::Register(vector_lane_mask_1),
            OperandType::Register(v_done),
            OperandType::Register(vector_pc),
            OperandType::Register(vector_pc),
        ],
    ));
    let jump_after_prune_1 = prefix.len();
    prefix.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(
            dispatch_head as i64 - jump_after_prune_1 as i64,
        )],
    ));

    let prune_lane_2 = prefix.len();
    prefix.push(Instruction::new(
        Opcode::VCmpGtI,
        vec![
            OperandType::Register(vector_lane_ids),
            OperandType::Register(vector_lane_bound_2),
            OperandType::Register(vector_lane_mask_2),
        ],
    ));
    prefix.push(Instruction::new(
        Opcode::VSelectI,
        vec![
            OperandType::Register(vector_lane_mask_2),
            OperandType::Register(v_done),
            OperandType::Register(vector_pc),
            OperandType::Register(vector_pc),
        ],
    ));
    let jump_after_prune_2 = prefix.len();
    prefix.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(
            dispatch_head as i64 - jump_after_prune_2 as i64,
        )],
    ));

    let all_done = prefix.len();
    prefix[all_done_branch].operands[2] =
        OperandType::Offset(all_done as i64 - all_done_branch as i64);

    // If any lane returned, select the first lane in scalar order. Otherwise,
    // process the next full batch; the scalar loop handles a 1-3 item tail.
    let mut completion_return_branches = Vec::<(usize, usize)>::new();
    for lane in 0..4usize {
        prefix.push(Instruction::new(
            Opcode::VExtractI,
            vec![
                OperandType::Register(vector_retmask),
                OperandType::Constant(Value::Int(lane as i64)),
                OperandType::Register(scalar_mask),
            ],
        ));
        let branch_idx = prefix.len();
        prefix.push(Instruction::new(
            Opcode::IEqJmpRC,
            vec![
                OperandType::Register(scalar_mask),
                OperandType::Constant(Value::Int(-1)),
                OperandType::Offset(0),
            ],
        ));
        completion_return_branches.push((branch_idx, lane));
    }
    prefix.push(Instruction::new(
        Opcode::IAddRCR,
        vec![
            OperandType::Register(induction),
            OperandType::Constant(Value::Int(SIMD_WIDTH)),
            OperandType::Register(induction),
        ],
    ));
    let batch_back = prefix.len();
    prefix.push(Instruction::new(
        Opcode::Jump,
        vec![OperandType::Offset(batch_head as i64 - batch_back as i64)],
    ));

    let mut return_labels = [0usize; 4];
    for lane in 0..4usize {
        return_labels[lane] = prefix.len();
        prefix.push(Instruction::new(
            Opcode::VExtractI,
            vec![
                OperandType::Register(vector_retvalue),
                OperandType::Constant(Value::Int(lane as i64)),
                OperandType::Register(scalar_retvalue),
            ],
        ));
        prefix.push(Instruction::new(
            Opcode::Ret,
            vec![OperandType::Register(scalar_retvalue)],
        ));
    }

    for (branch_idx, lane) in completion_return_branches {
        prefix[branch_idx].operands[2] =
            OperandType::Offset(return_labels[lane] as i64 - branch_idx as i64);
    }
    for idx in immediate_return_branches {
        prefix[idx].operands[2] = OperandType::Offset(return_labels[0] as i64 - idx as i64);
    }
    for idx in prune_lane_1_branches {
        prefix[idx].operands[2] = OperandType::Offset(prune_lane_1 as i64 - idx as i64);
    }
    for idx in prune_lane_2_branches {
        prefix[idx].operands[2] = OperandType::Offset(prune_lane_2 as i64 - idx as i64);
    }
    let fallback_pc = prefix.len();
    for idx in fallback_branches {
        prefix[idx].operands[1] = OperandType::Offset(fallback_pc as i64 - idx as i64);
    }

    Some((prefix, next_reg))
}

fn vectorize_loops_with_types(
    func: &mut Function,
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
    safe_readonly_array_args: &HashSet<usize>,
) -> bool {
    // Scalar typed arithmetic may panic on overflow in debug builds while the
    // current vector helpers wrap. Keep every loop scalar until both execution
    // paths share the same overflow contract.
    if cfg!(debug_assertions) {
        return false;
    }
    let mut changed = false;
    let mut scan_from = 0usize;

    loop {
        if scan_from >= func.instructions.len() {
            break;
        }
        let analysis = analyze_types(func, returns, argument_types);
        let mut transformed = false;
        let mut pc = scan_from;
        while pc < func.instructions.len() {
            let inst = &func.instructions[pc];
            if inst.opcode == Opcode::Jump {
                if let Some(OperandType::Offset(off)) = inst.operands.first() {
                    if *off < 0 {
                        if let Some(head) = target_from_offset(pc, *off, func.instructions.len()) {
                            if head < pc {
                                let candidate = build_predicated_simd_prefix(
                                    func,
                                    head,
                                    pc,
                                    &analysis,
                                    argument_types,
                                    safe_readonly_array_args,
                                )
                                .or_else(|| build_int_simd_prefix(func, head, pc, &analysis));
                                if let Some((prefix, new_count)) = candidate {
                                    let prefix_len = prefix.len();
                                    if insert_prefix_with_relocation(func, head, prefix, new_count)
                                    {
                                        changed = true;
                                        // Skip the transformed scalar loop so it is
                                        // not vectorized a second time on this run.
                                        scan_from = pc.saturating_add(prefix_len).saturating_add(1);
                                        transformed = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            pc += 1;
        }
        if !transformed {
            break;
        }
    }
    changed
}

/// Optimize one function. LLVM/native compilation is intentionally outside this
/// module; this function only rewrites bytecode before the VM's later JIT stage.
pub fn optimize_function(func: &mut Function) {
    optimize_function_with_types_and_args(
        func,
        &HashMap::new(),
        &unknown_argument_types(func),
        &HashSet::new(),
    );
}

fn direct_callee_name(bytecode: &Bytecode, inst: &Instruction) -> Option<String> {
    match inst.opcode {
        Opcode::Call | Opcode::Spawn => match inst.operands.first()? {
            OperandType::Function(name) => Some(name.clone()),
            _ => None,
        },
        Opcode::CallJit => {
            let OperandType::Offset(index) = inst.operands.first()? else {
                return None;
            };
            if *index < 0 {
                return None;
            }
            bytecode
                .functions
                .get(*index as usize)
                .map(|f| f.name.clone())
        }
        _ => None,
    }
}

fn direct_call_argument_count(inst: &Instruction) -> Option<usize> {
    if !matches!(inst.opcode, Opcode::Call | Opcode::CallJit | Opcode::Spawn) {
        return None;
    }
    match inst.operands.get(1)? {
        OperandType::Offset(count) if *count >= 0 => Some(*count as usize),
        _ => None,
    }
}

fn infer_argument_types(
    bytecode: &Bytecode,
    returns: &HashMap<String, ValType>,
    current: &HashMap<String, Vec<ValType>>,
) -> HashMap<String, Vec<ValType>> {
    let mut next: HashMap<String, Vec<ValType>> = bytecode
        .functions
        .iter()
        .map(|f| (f.name.clone(), vec![ValType::Unknown; f.argument_count]))
        .collect();
    let mut observed: HashMap<String, Vec<Option<ValType>>> = bytecode
        .functions
        .iter()
        .map(|f| (f.name.clone(), vec![None; f.argument_count]))
        .collect();
    let mut conflicted: HashMap<String, Vec<bool>> = bytecode
        .functions
        .iter()
        .map(|f| (f.name.clone(), vec![false; f.argument_count]))
        .collect();
    let mut escaped = HashSet::<String>::new();

    // A Function operand used as a value/closure can be invoked without a
    // statically visible argument list. Do not infer that target's parameters.
    for func in &bytecode.functions {
        for inst in &func.instructions {
            for (idx, op) in inst.operands.iter().enumerate() {
                if let OperandType::Function(name) = op {
                    let is_direct_target =
                        idx == 0 && matches!(inst.opcode, Opcode::Call | Opcode::Spawn);
                    if !is_direct_target {
                        escaped.insert(name.clone());
                    }
                }
            }
        }
    }
    // The module entry point may be invoked by the host with unknown arguments.
    if let Some(entry) = bytecode.functions.first() {
        escaped.insert(entry.name.clone());
    }
    if let Some(main) = bytecode.functions.iter().find(|f| f.name == "main") {
        escaped.insert(main.name.clone());
    }

    for caller in &bytecode.functions {
        let caller_args = current
            .get(&caller.name)
            .cloned()
            .unwrap_or_else(|| unknown_argument_types(caller));
        let analysis = analyze_types(caller, returns, &caller_args);
        for (pc, inst) in caller.instructions.iter().enumerate() {
            let Some(callee) = direct_callee_name(bytecode, inst) else {
                continue;
            };
            let Some(argc) = direct_call_argument_count(inst) else {
                escaped.insert(callee);
                continue;
            };
            let Some(arg_slots) = observed.get_mut(&callee) else {
                continue;
            };
            let Some(conflict_slots) = conflicted.get_mut(&callee) else {
                continue;
            };
            let state = analysis.before.get(pc).and_then(|s| s.as_ref());
            for arg_idx in 0..arg_slots.len() {
                if arg_idx >= argc {
                    conflict_slots[arg_idx] = true;
                    continue;
                }
                let ty = match state {
                    Some(state) => inst
                        .operands
                        .get(2 + arg_idx)
                        .map(|op| operand_type_with_args(op, state, &caller_args))
                        .unwrap_or(ValType::Unknown),
                    None => ValType::Unknown,
                };
                if ty == ValType::Unknown {
                    conflict_slots[arg_idx] = true;
                    continue;
                }
                match arg_slots[arg_idx] {
                    None => arg_slots[arg_idx] = Some(ty),
                    Some(existing) if existing == ty => {}
                    Some(_) => conflict_slots[arg_idx] = true,
                }
            }
            // A target with fewer arguments than its declared arity cannot have
            // a single proven type signature for this call graph.
            if argc < arg_slots.len() {
                for conflict in conflict_slots.iter_mut() {
                    *conflict = true;
                }
            }
        }
    }

    for func in &bytecode.functions {
        let Some(dst) = next.get_mut(&func.name) else {
            continue;
        };
        let Some(obs) = observed.get(&func.name) else {
            continue;
        };
        let Some(conflicts) = conflicted.get(&func.name) else {
            continue;
        };
        if escaped.contains(&func.name) {
            continue;
        }
        for idx in 0..dst.len() {
            if !conflicts[idx] {
                if let Some(ty) = obs[idx] {
                    dst[idx] = ty;
                }
            }
        }
    }
    next
}

fn optimize_function_with_types_and_args(
    func: &mut Function,
    func_return_types: &HashMap<String, ValType>,
    argument_types: &[ValType],
    safe_readonly_array_args: &HashSet<usize>,
) {
    let _ = remove_redundant_copy_roundtrips(func);
    let _ = specialize_types(func, func_return_types, argument_types);
    let _ = unroll_small_constant_loops(func);
    let _ = specialize_types(func, func_return_types, argument_types);
    let _ = vectorize_loops_with_types(
        func,
        func_return_types,
        argument_types,
        safe_readonly_array_args,
    );
    let final_analysis = analyze_types(func, func_return_types, argument_types);
    if func.return_type.is_none() {
        if let Some(ret) = final_analysis.inferred_return {
            func.return_type = Some(ret);
        }
    }
}

/// Infer internal argument and return types to a fixed point, then optimize
/// each function once. Argument types only become facts when every statically
/// visible call site agrees and the function does not escape as a first-class
/// value; otherwise they remain Unknown.
pub fn optimize_bytecode(bytecode: &mut Bytecode) {
    let mut returns: HashMap<String, ValType> = bytecode
        .functions
        .iter()
        .filter_map(|f| f.return_type.map(|ty| (f.name.clone(), ty)))
        .collect();
    let mut argument_types: HashMap<String, Vec<ValType>> = bytecode
        .functions
        .iter()
        .map(|f| (f.name.clone(), unknown_argument_types(f)))
        .collect();

    for _ in 0..=bytecode.functions.len().saturating_mul(2).saturating_add(2) {
        let next_args = infer_argument_types(bytecode, &returns, &argument_types);
        let mut changed = next_args != argument_types;
        argument_types = next_args;

        for func in &bytecode.functions {
            if returns.contains_key(&func.name) {
                continue;
            }
            let args = argument_types
                .get(&func.name)
                .cloned()
                .unwrap_or_else(|| unknown_argument_types(func));
            let analysis = analyze_types(func, &returns, &args);
            if let Some(ret) = analysis.inferred_return {
                returns.insert(func.name.clone(), ret);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let local_readonly = local_readonly_array_parameters(bytecode);
    let safe_array_args =
        array_argument_origins_are_safe(bytecode, &returns, &argument_types, &local_readonly);

    for func in &mut bytecode.functions {
        let args = argument_types
            .get(&func.name)
            .cloned()
            .unwrap_or_else(|| unknown_argument_types(func));
        let safe_arrays = safe_array_args.get(&func.name).cloned().unwrap_or_default();
        optimize_function_with_types_and_args(func, &returns, &args, &safe_arrays);
    }
}

/// Public compatibility entry point retained for callers that already know
/// callee return types but cannot provide whole-bytecode argument facts.
pub fn optimize_function_with_types(
    func: &mut Function,
    func_return_types: &HashMap<String, ValType>,
) {
    let args = unknown_argument_types(func);
    optimize_function_with_types_and_args(func, func_return_types, &args, &HashSet::new());
}

/// Attempt conservative four-lane vectorization of supported integer-array loops.
/// This includes a predicated CFG path for pure nested loops when the argument
/// provenance and every operation's legality can be proved. Unsupported patterns
/// remain unchanged and continue to execute using the original scalar bytecode.
pub fn vectorize_loops(func: &mut Function) -> bool {
    vectorize_loops_with_types(
        func,
        &HashMap::new(),
        &unknown_argument_types(func),
        &HashSet::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_argument_count_is_not_treated_as_branch_target() {
        let call = Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("callee".to_string()),
                OperandType::Offset(3),
                OperandType::Register(0),
                OperandType::Register(1),
                OperandType::Register(2),
                OperandType::Register(3),
            ],
        );
        assert_eq!(jump_offset_index(&call), None);
        assert!(all_offsets_are_relocatable(&Function::new(
            "f".to_string(),
            vec![call],
            4,
            0,
            crate::lang::types::FunctionType::Regular,
        )));
    }

    #[test]
    fn insertion_relocates_branches_from_before_the_insertion_point() {
        let mut f = Function::new(
            "branch_over_prefix".to_string(),
            vec![
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(0), OperandType::Offset(2)],
                ),
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(0))]),
            ],
            1,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(insert_prefix_with_relocation(
            &mut f,
            1,
            vec![
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(Opcode::Nop, vec![])
            ],
            1,
        ));
        // Old target pc 2 is now pc 4, so the branch at pc 0 needs offset 4.
        assert_eq!(f.instructions[0].operands[1], OperandType::Offset(4));
    }

    #[test]
    fn unknown_call_result_invalidates_destination_type() {
        let mut state = vec![ValType::Int; 4];
        let call = Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("dynamic".to_string()),
                OperandType::Offset(0),
                OperandType::Register(0),
            ],
        );
        transfer_types(&call, &mut state, &HashMap::new(), &[]);
        assert_eq!(state[0], ValType::Unknown);
        assert_eq!(state[1], ValType::Int);
    }

    #[test]
    fn receive_invalidates_register_type_facts() {
        // Receive-style operations may write a destination register, and this
        // module intentionally does not assume their operand schema.
        for opcode in [Opcode::Receive, Opcode::ReceiveTimeout, Opcode::Spawn] {
            let mut state = vec![ValType::Int, ValType::Float, ValType::String];
            let inst = Instruction::new(opcode, vec![]);
            transfer_types(&inst, &mut state, &HashMap::new(), &[]);
            assert!(state.iter().all(|ty| *ty == ValType::Unknown));
        }
    }

    #[test]
    fn small_constant_loop_is_unrolled_sequentially() {
        // r2 = 0; for r0 in 0..3 { r2 += r0; r0 += 1 }; return r2
        let mut f = Function::new(
            "sum3".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::Lt,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(3)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(4)],
                ),
                Instruction::new(
                    Opcode::Add,
                    vec![
                        OperandType::Register(2),
                        OperandType::Register(0),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::Add,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
            ],
            3,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(unroll_small_constant_loops(&mut f));
        assert_eq!(f.instructions.len(), 9);
        assert_eq!(
            f.instructions
                .iter()
                .filter(|i| i.opcode == Opcode::Add)
                .count(),
            6
        );
        assert!(
            f.instructions
                .iter()
                .all(|i| i.opcode != Opcode::Jump && i.opcode != Opcode::JumpFalse)
        );
        assert!(matches!(f.instructions.last().unwrap().opcode, Opcode::Ret));
    }

    #[test]
    fn unrolled_early_return_branches_stay_within_each_iteration_copy() {
        // for r0 in 0..3 { if r0 == 1 { return r0 }; r0 += 1 }
        let mut f = Function::new(
            "early_return".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::Lt,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(3)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(7)],
                ),
                Instruction::new(
                    Opcode::IEqRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(2), OperandType::Offset(2)],
                ),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-7)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
            ],
            3,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(unroll_small_constant_loops(&mut f));
        let branch_pcs = [2usize, 7, 12];
        for pc in branch_pcs {
            assert_eq!(f.instructions[pc].opcode, Opcode::JumpFalse);
            assert_eq!(
                direct_branch_target(&f.instructions[pc], pc, f.instructions.len()),
                Some(pc + 2)
            );
        }
        assert_eq!(f.instructions.len(), 17);
    }

    #[test]
    fn unroll_relocates_branches_before_the_loop() {
        // Branch at pc 0 targets pc 7 (the return), skipping the loop. Once the
        // loop shrinks, the branch displacement must be adjusted too.
        let mut f = Function::new(
            "branch_over_loop".to_string(),
            vec![
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(3), OperandType::Offset(7)],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::Lt,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(2)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(4)],
                ),
                Instruction::new(
                    Opcode::Add,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::Add,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
            ],
            4,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(unroll_small_constant_loops(&mut f));
        assert_eq!(f.instructions[0].operands[1], OperandType::Offset(6));
        assert_eq!(f.instructions.len(), 7);
    }

    #[test]
    fn unroll_preserves_loop_condition_register_live_out_by_bailing_out() {
        let mut f = Function::new(
            "condition_live_out".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::Lt,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(2)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(4)],
                ),
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(
                    Opcode::Add,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
            ],
            3,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(!unroll_small_constant_loops(&mut f));
        assert!(
            f.instructions
                .iter()
                .any(|inst| inst.opcode == Opcode::Jump)
        );
    }

    #[test]
    fn simd_prefix_falls_back_to_scalar_loop_for_tail_iterations() {
        // The vector prefix must leave the original scalar loop in place. At
        // runtime a limit of 5, 6, or 7 executes the last 1, 2, or 3 iterations
        // through that original loop rather than performing an out-of-range gather.
        let mut f = Function::new(
            "simd_tail".to_string(),
            vec![
                Instruction::new(
                    Opcode::NewIntArray,
                    vec![
                        OperandType::Constant(Value::Int(8)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::Lt,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(8)),
                        OperandType::Register(3),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(3), OperandType::Offset(5)],
                ),
                Instruction::new(
                    Opcode::IArrayGet,
                    vec![
                        OperandType::Register(1),
                        OperandType::Register(0),
                        OperandType::Register(4),
                    ],
                ),
                Instruction::new(
                    Opcode::IArraySet,
                    vec![
                        OperandType::Register(1),
                        OperandType::Register(0),
                        OperandType::Register(4),
                    ],
                ),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-5)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
            ],
            5,
            0,
            crate::lang::types::FunctionType::Regular,
        );

        let analysis = analyze_types(&f, &HashMap::new(), &unknown_argument_types(&f));
        let (prefix, new_register_count) = build_int_simd_prefix(&f, 2, 7, &analysis)
            .expect("canonical local integer-array loop should be eligible");
        let prefix_len = prefix.len();
        let exit_idx = prefix
            .iter()
            .position(|inst| inst.opcode == Opcode::IJumpFalseR)
            .expect("vector-width guard should have a scalar fallback");
        let exit_off = match prefix[exit_idx].operands.get(1) {
            Some(OperandType::Offset(off)) => *off,
            other => panic!("missing vector exit offset: {other:?}"),
        };
        assert_eq!(
            target_from_offset(exit_idx, exit_off, prefix_len),
            Some(prefix_len)
        );

        assert!(insert_prefix_with_relocation(
            &mut f,
            2,
            prefix,
            new_register_count
        ));
        assert_eq!(f.instructions[2 + prefix_len].opcode, Opcode::Lt);
        assert_eq!(
            direct_branch_target(
                &f.instructions[3 + prefix_len],
                3 + prefix_len,
                f.instructions.len()
            ),
            Some(8 + prefix_len)
        );
    }

    #[test]
    fn predicated_region_rejects_an_escape_back_into_the_scalar_loop() {
        // A post-backedge target is not automatically a valid lane-local exit.
        // If it jumps back to the scalar loop, vectorization must conservatively
        // decline instead of accidentally executing a second scalar iteration.
        let f = Function::new(
            "unsafe_escape".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::ILtRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(4)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(6)],
                ),
                Instruction::new(
                    Opcode::IArrayGet,
                    vec![
                        OperandType::Argument(0),
                        OperandType::Register(0),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::IGtRCR,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(3),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(3), OperandType::Offset(4)],
                ),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-6)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),
            ],
            4,
            1,
            crate::lang::types::FunctionType::Regular,
        );

        assert!(predicated_loop_region(&f, 1, 7).is_none());
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_rejects_explicit_branch_to_outer_latch() {
        // A jump straight to the latch executes the scalar increment and then
        // re-enters the loop. It is not equivalent to marking the SIMD lane done.
        let f = Function::new(
            "explicit_latch_branch".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::ILtRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(4)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(8)],
                ),
                Instruction::new(
                    Opcode::IArrayGet,
                    vec![
                        OperandType::Argument(0),
                        OperandType::Register(0),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::IGtRCR,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(3),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(3), OperandType::Offset(6)],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(2)]), // Explicit branch to latch at pc 8.
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
            ],
            4,
            1,
            crate::lang::types::FunctionType::Regular,
        );

        let args = [ValType::IntArray];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([0usize]);
        assert!(
            build_predicated_simd_prefix(&f, 1, 9, &analysis, &args, &safe_array_args).is_none()
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_accepts_return_epilogue_liveins_written_only_after_backedge() {
        // The post-backedge epilogue writes r4, but the scalar loop-exhaustion
        // return also reads r4. Those paths are mutually exclusive: writes in
        // the return-only epilogue must not be mistaken for loop-carried writes.
        let mut f = Function::new(
            "return_epilogue_livein".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(-1)),
                        OperandType::Register(4),
                    ],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Argument(1), OperandType::Register(1)],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::ILtRRR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Register(1),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(2), OperandType::Offset(6)],
                ),
                Instruction::new(
                    Opcode::IArrayGet,
                    vec![
                        OperandType::Argument(0),
                        OperandType::Register(0),
                        OperandType::Register(3),
                    ],
                ),
                Instruction::new(
                    Opcode::IGtRCR,
                    vec![
                        OperandType::Register(3),
                        OperandType::Constant(Value::Int(100)),
                        OperandType::Register(5),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(5), OperandType::Offset(5)],
                ),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-6)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
                Instruction::new(Opcode::Nop, vec![]),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Register(4), OperandType::Register(6)],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(1)]),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Register(6), OperandType::Register(4)],
                ),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
            ],
            7,
            2,
            crate::lang::types::FunctionType::Regular,
        );

        let args = [ValType::IntArray, ValType::Unknown];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([0usize]);
        let region = predicated_loop_region(&f, 3, 9)
            .expect("return-only epilogue after the loop should be accepted");
        assert!(region.contains(&12));
        assert!(region.contains(&13));
        assert!(region.contains(&14));
        assert!(region.contains(&15));
        assert!(
            !region.contains(&10),
            "normal loop exhaustion must remain scalar"
        );

        let (prefix, new_register_count) =
            build_predicated_simd_prefix(&f, 3, 9, &analysis, &args, &safe_array_args)
                .expect("tail-only register writes must not be treated as loop-carried writes");
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VSelectI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VGatherI));
        let prefix_len = prefix.len();
        assert!(insert_prefix_with_relocation(
            &mut f,
            3,
            prefix,
            new_register_count
        ));
        assert_eq!(f.instructions[3 + prefix_len].opcode, Opcode::ILtRRR);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_accepts_a_nested_integer_array_search_loop() {
        // General nested-loop shape (not a Munchausen intrinsic): candidates
        // run from 0..8; each candidate repeatedly divides by ten, gathers from
        // a read-only IntArray, accumulates a sum, and can break its inner loop
        // early when the sum exceeds the candidate. Matching candidates return
        // from the scalar loop in ascending order.
        let mut f = Function::new(
            "nested_search".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(
                    Opcode::ILtRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(8)),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(1), OperandType::Offset(21)],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Register(0), OperandType::Register(2)],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(3),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(1)]),
                Instruction::new(
                    Opcode::IGtRCR,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(0)),
                        OperandType::Register(4),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpFalse,
                    vec![OperandType::Register(4), OperandType::Offset(11)],
                ),
                Instruction::new(
                    Opcode::IRemRCR,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(10)),
                        OperandType::Register(5),
                    ],
                ),
                Instruction::new(
                    Opcode::IArrayGet,
                    vec![
                        OperandType::Argument(2),
                        OperandType::Register(5),
                        OperandType::Register(6),
                    ],
                ),
                Instruction::new(
                    Opcode::IAddRRR,
                    vec![
                        OperandType::Register(3),
                        OperandType::Register(6),
                        OperandType::Register(7),
                    ],
                ),
                Instruction::new(
                    Opcode::IGtRRR,
                    vec![
                        OperandType::Register(7),
                        OperandType::Register(0),
                        OperandType::Register(8),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(8), OperandType::Offset(3)],
                ),
                Instruction::new(
                    Opcode::IDivRCR,
                    vec![
                        OperandType::Register(2),
                        OperandType::Constant(Value::Int(10)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(2)]),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![
                        OperandType::Constant(Value::Int(-1)),
                        OperandType::Register(2),
                    ],
                ),
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Register(7), OperandType::Register(3)],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-11)]),
                Instruction::new(
                    Opcode::IEqRRR,
                    vec![
                        OperandType::Register(3),
                        OperandType::Register(0),
                        OperandType::Register(9),
                    ],
                ),
                Instruction::new(
                    Opcode::JumpTrue,
                    vec![OperandType::Register(9), OperandType::Offset(3)],
                ),
                Instruction::new(
                    Opcode::IAddRCR,
                    vec![
                        OperandType::Register(0),
                        OperandType::Constant(Value::Int(1)),
                        OperandType::Register(0),
                    ],
                ),
                Instruction::new(Opcode::Jump, vec![OperandType::Offset(-20)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
            ],
            10,
            3,
            crate::lang::types::FunctionType::Regular,
        );

        let args = [ValType::Int, ValType::Unknown, ValType::IntArray];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([2usize]);
        let (prefix, new_register_count) =
            build_predicated_simd_prefix(&f, 1, 21, &analysis, &args, &safe_array_args)
                .expect("nested pure integer-array loop should be eligible in release mode");
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VSelectI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VGatherI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VRemI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VDivI));
        let prefix_len = prefix.len();
        assert!(insert_prefix_with_relocation(
            &mut f,
            1,
            prefix,
            new_register_count
        ));
        // The unmodified scalar loop remains as the fallback/tail after the
        // generated SIMD prefix, with all original branches relocated.
        assert_eq!(f.instructions[1 + prefix_len].opcode, Opcode::ILtRCR);
        for (pc, inst) in f.instructions.iter().enumerate() {
            for (operand_idx, op) in inst.operands.iter().enumerate() {
                if let OperandType::Offset(off) = op {
                    if offset_is_non_branch_metadata(inst.opcode, operand_idx) {
                        continue;
                    }
                    assert_eq!(
                        jump_offset_index(inst),
                        Some(operand_idx),
                        "unexpected Offset at pc {pc}"
                    );
                    assert!(
                        target_from_offset(pc, *off, f.instructions.len()).is_some(),
                        "invalid target at pc {pc}"
                    );
                }
            }
        }
    }

    #[test]
    fn call_offsets_do_not_change_during_insert_relocation() {
        let call = Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("callee".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(1),
                OperandType::Register(2),
            ],
        );
        let mut f = Function::new(
            "f".to_string(),
            vec![
                Instruction::new(Opcode::Nop, vec![]),
                call,
                Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(0))]),
            ],
            3,
            0,
            crate::lang::types::FunctionType::Regular,
        );
        let prefix = vec![Instruction::new(Opcode::Nop, vec![])];
        assert!(insert_prefix_with_relocation(&mut f, 1, prefix, 3));
        match f.instructions[2].operands.get(1) {
            Some(OperandType::Offset(2)) => {}
            other => panic!("call arity changed: {other:?}"),
        }
    }
}
