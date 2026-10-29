//! Predicated CFG-to-SIMD lowering for supported pure integer loop regions.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::bytecode::instruction::{Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};

use super::analysis::*;
use super::array_analysis::*;
use super::loop_transforms::*;
use super::SIMD_WIDTH;

// General predicated CFG-to-SIMD lowering for pure integer loop regions. Each lane
// carries a virtual bytecode PC, so branches and nested loops remain lane-local.
// The outer loop's latch ends one candidate iteration; branch targets past the
// loop are admitted only when every path through that epilogue ends in Ret.
// This deliberately supports a small, explicitly audited opcode subset and
// requires a range proof for every gather index before it will emit VGatherI.
pub(super) fn predicated_loop_region(func: &Function, head: usize, back: usize) -> Option<Vec<usize>> {
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
        || !seen.iter().any(|pc| func.instructions[*pc].opcode == Opcode::Ret)
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

pub(super) fn is_int_operand(op: &OperandType, state: &[ValType], args: &[ValType]) -> bool {
    operand_type_with_args(op, state, args) == ValType::Int
}

pub(super) fn is_int_register_operand(op: Option<&OperandType>, state: &[ValType]) -> bool {
    match op {
        Some(OperandType::Register(r)) => state.get(*r).copied() == Some(ValType::Int),
        _ => false,
    }
}

pub(super) fn typed_int_operand_compatible(
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

pub(super) fn predicated_integer_demand(
    func: &Function,
    region: &[usize],
    induction: usize,
    limit: &OperandType,
) -> HashSet<usize> {
    let mut demand = HashSet::from([induction]);
    if let OperandType::Register(r) = limit { demand.insert(*r); }

    // Explicit typed integer instructions establish integer requirements for
    // their value operands. Do not infer types from generic dynamic opcodes.
    for &pc in region {
        let inst = &func.instructions[pc];
        let integer_indices: &[usize] = match inst.opcode {
            Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR
            | Opcode::IEqRRR | Opcode::ILtRRR | Opcode::IGtRRR
            | Opcode::IGtJmpRR | Opcode::ILtJmpRR | Opcode::IEqJmpRR => &[0, 1],
            Opcode::IAddRCR | Opcode::ISubRCR | Opcode::IMulRCR
            | Opcode::IDivRCR | Opcode::IRemRCR
            | Opcode::IEqRCR | Opcode::ILtRCR | Opcode::ILtCRR
            | Opcode::IGtRCR | Opcode::IGtCRR
            | Opcode::IGtJmpRC | Opcode::ILtJmpRC | Opcode::IEqJmpRC => &[0],
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
            if inst.opcode != Opcode::LoadConst { continue; }
            let Some(OperandType::Register(dst)) = inst.operands.get(1) else { continue };
            let Some(OperandType::Register(src)) = inst.operands.first() else { continue };
            if demand.contains(dst) && demand.insert(*src) { changed = true; }
        }
        if !changed { break; }
    }
    demand
}

pub(super) fn predicated_instruction_is_supported(
    inst: &Instruction,
    state: &[ValType],
    argument_types: &[ValType],
    int_demand: &HashSet<usize>,
    induction: usize,
) -> bool {
    let ops = &inst.operands;
    let int_binary = |i: usize| ops.get(i).map(|o| is_int_operand(o, state, argument_types)).unwrap_or(false);
    let typed_int_binary = |i: usize| ops.get(i)
        .map(|o| typed_int_operand_compatible(o, state, argument_types, int_demand))
        .unwrap_or(false);

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

pub(super) fn predicated_vector_opcode(op: Opcode) -> Option<Opcode> {
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

pub(super) fn predicated_vector_operand(
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
        OperandType::Register(r) if *r == induction => Some(OperandType::Register(vector_induction)),
        OperandType::Register(r) => vector_registers.get(r).copied().map(OperandType::Register),
        OperandType::Constant(Value::Int(v)) => {
            let reg = if let Some(reg) = constant_vectors.get(v) {
                *reg
            } else {
                let reg = alloc_temp(next_reg);
                emitted.push(Instruction::new(Opcode::VSplatI, vec![
                    OperandType::Constant(Value::Int(*v)),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(reg),
                ]));
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
                emitted.push(Instruction::new(Opcode::VSplatI, vec![
                    OperandType::Constant(Value::Int(scalar)),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(reg),
                ]));
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
                emitted.push(Instruction::new(Opcode::VSplatI, vec![
                    OperandType::Argument(*a),
                    OperandType::Constant(Value::Int(SIMD_WIDTH)),
                    OperandType::Register(reg),
                ]));
                argument_vectors.insert(*a, reg);
                reg
            };
            Some(OperandType::Register(reg))
        }
        _ => None,
    }
}

pub(super) fn predicated_emit_pc_update(
    out: &mut Vec<Instruction>,
    exec_mask: usize,
    target_vec: usize,
    vector_pc: usize,
) {
    out.push(Instruction::new(Opcode::VSelectI, vec![
        OperandType::Register(exec_mask),
        OperandType::Register(target_vec),
        OperandType::Register(vector_pc),
        OperandType::Register(vector_pc),
    ]));
}

#[derive(Clone)]
pub enum ArrayLengthRequirement {
    AtLeastConstant(i64),
    AtLeastOperand(OperandType),
}

pub(super) fn required_array_length_for_index(
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
            return Some(ArrayLengthRequirement::AtLeastConstant((*value).max(0).checked_add(1)?));
        }
        OperandType::Register(index_reg) => {
            let defs: Vec<(usize, &Instruction)> = region.iter().filter_map(|pc| {
                let inst = &func.instructions[*pc];
                instruction_written_registers(inst).contains(index_reg).then_some((*pc, inst))
            }).collect();
            if defs.len() != 1 { return None; }
            let (def_pc, def) = defs[0];
            if def.opcode != Opcode::IRemRCR
                || is_register(def.operands.first()).is_none()
                || is_register(def.operands.get(2)) != Some(*index_reg)
            {
                return None;
            }
            let divisor = constant_int(def.operands.get(1)?)?;
            if divisor <= 0 { return None; }
            let dividend = is_register(def.operands.first())?;

            // Prove that this modulo executes only on the nonnegative path:
            // `dividend > 0`, immediately followed by a false-branch over the
            // modulo. This covers digit loops and avoids assuming Rust `%`
            // bounds for a negative dividend.
            if def_pc < 2 { return None; }
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
                if source_pc != branch_pc
                    && successors(func, source_pc).contains(&def_pc)
                {
                    return None;
                }
            }
            return Some(ArrayLengthRequirement::AtLeastConstant(divisor));
        }
        _ => None,
    }
}

pub(super) fn build_predicated_simd_prefix(
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
    if cfg!(debug_assertions) { return None; }
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
    if is_register(branch.operands.first())? != condition { return None; }
    let limit = header.operands.get(1)?.clone();
    if !matches!(header.opcode, Opcode::ILtRRR) && !matches!(&limit, OperandType::Constant(Value::Int(_))) {
        return None;
    }
    if matches!(&limit, OperandType::Constant(Value::Int(v)) if *v < 0) { return None; }
    let head_state = types.before.get(head)?.as_ref()?;
    let induction_ty = head_state.get(induction).copied().unwrap_or(ValType::Unknown);
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
    if target_from_offset(back, match instrs[back].operands.first()? {
        OperandType::Offset(off) => *off,
        _ => return None,
    }, instrs.len())? != head {
        return None;
    }
    let exit_target = target_from_offset(head + 1, match branch.operands.get(1)? {
        OperandType::Offset(off) => *off,
        _ => return None,
    }, instrs.len())?;

    let region = predicated_loop_region(func, head, back)?;
    let region_set: HashSet<usize> = region.iter().copied().collect();
    if region_set.contains(&exit_target) { return None; }
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
        if pc == latch { continue; }
        let inst = &instrs[pc];
        let state = types.before.get(pc)?.as_ref()?;
        if !predicated_instruction_is_supported(inst, state, argument_types, &int_demand, induction) { return None; }
        if inst.opcode == Opcode::IArrayGet {
            has_array_read = true;
            let arr = inst.operands.first()?.clone();
            let index = inst.operands.get(1)?;
            let requirement = required_array_length_for_index(func, &region, induction, &limit, index)?;
            array_length_requirements.push((arr.clone(), requirement));
            if !array_base_ops.contains(&arr) { array_base_ops.push(arr.clone()); }
            if let OperandType::Register(r) = arr { array_base_regs.insert(r); }
        }
        let r: HashSet<usize> = instruction_reads(inst).into_iter().collect();
        let w: HashSet<usize> = instruction_written_registers(inst).into_iter().collect();
        for reg in r.iter().chain(w.iter()) { all_regs.insert(*reg); }
        reads.insert(pc, r);
        writes.insert(pc, w);

        // The induction value belongs to the batch, not the scalar loop body.
        // A body write to it would invalidate the lane-to-candidate mapping.
        if instruction_written_registers(inst).contains(&induction) { return None; }
    }
    if !has_array_read { return None; }
    // Distinct arrays may be read, but every base must be a proven IntArray.
    // `array_base_ops` stays scalar; all numeric/index registers are lane-mapped.
    for arr in &array_base_ops {
        let state = types.before.get(head + 2)?.as_ref()?;
        if operand_type_with_args(arr, state, argument_types) != ValType::IntArray { return None; }
        let readonly_proven = match arr {
            OperandType::Argument(arg) => safe_readonly_array_args.contains(arg),
            OperandType::Register(reg) => {
                is_private_array_register(func, *reg, head, back)
                    || array_argument_origin_before(func, *reg, head)
                        .map(|arg| safe_readonly_array_args.contains(&arg)).unwrap_or(false)
            }
            _ => false,
        };
        if !readonly_proven { return None; }
    }

    // Only writes on the repeating candidate path can be loop-carried or leak
    // into the scalar continuation. Writes in the post-backedge return-only
    // epilogue are lane-local: those paths terminate in Ret and cannot reach the
    // next candidate or the normal loop-exhaustion return. Include their
    // registers in `all_regs`/liveness, but do not mistake them for loop writes.
    let mut loop_writes = HashSet::<usize>::new();
    for (&pc, ws) in &writes {
        if pc == latch || pc > back { continue; }
        loop_writes.extend(ws.iter().copied());
    }
    for reg in &loop_writes {
        if *reg == induction { return None; }
        for pc in (head + 1)..instrs.len() {
            if region_set.contains(&pc) { continue; }
            if instruction_reads(&instrs[pc]).contains(reg) { return None; }
        }
    }
    // The loop limit must be invariant across every candidate iteration.
    if let OperandType::Register(limit_reg) = &limit {
        if loop_writes.contains(limit_reg) { return None; }
    }

    // Compute standard CFG liveness over the candidate body. Every live-in is
    // broadcast from the scalar register at the start of each four-item batch.
    let mut live_in: HashMap<usize, HashSet<usize>> = region.iter().map(|pc| (*pc, HashSet::new())).collect();
    let mut live_out: HashMap<usize, HashSet<usize>> = region.iter().map(|pc| (*pc, HashSet::new())).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for &pc in region.iter().rev() {
            let mut out = HashSet::new();
            if pc != latch && instrs[pc].opcode != Opcode::Ret {
                for succ in successors(func, pc) {
                    if region_set.contains(&succ) {
                        if let Some(s) = live_in.get(&succ) { out.extend(s.iter().copied()); }
                    }
                }
            }
            let mut input = reads.get(&pc).cloned().unwrap_or_default();
            let defs = writes.get(&pc).cloned().unwrap_or_default();
            input.extend(out.difference(&defs).copied());
            if live_out.get(&pc) != Some(&out) { live_out.insert(pc, out); changed = true; }
            if live_in.get(&pc) != Some(&input) { live_in.insert(pc, input); changed = true; }
        }
    }
    let live_ins = live_in.get(&(head + 2))?;
    for r in live_ins {
        if *r == induction || array_base_regs.contains(r) { continue; }
        if loop_writes.contains(r) { return None; }
        let ty = types.before.get(head + 2)?.as_ref()?.get(*r).copied().unwrap_or(ValType::Unknown);
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
            if !matches!(state.get(*r).copied().unwrap_or(ValType::Unknown), ValType::Int | ValType::Bool | ValType::Unknown) {
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
        if *r == induction || array_base_regs.contains(r) { continue; }
        vector_registers.insert(*r, alloc_temp(&mut next_reg));
    }

    let mut prefix = Vec::<Instruction>::new();
    let mut const_vectors = HashMap::<i64, usize>::new();
    let mut arg_vectors = HashMap::<usize, usize>::new();
    let v_zero = {
        let r = alloc_temp(&mut next_reg);
        prefix.push(Instruction::new(Opcode::VSplatI, vec![
            OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(r),
        ]));
        const_vectors.insert(0, r);
        r
    };
    let v_minus_one = {
        let r = alloc_temp(&mut next_reg);
        prefix.push(Instruction::new(Opcode::VSplatI, vec![
            OperandType::Constant(Value::Int(-1)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(r),
        ]));
        const_vectors.insert(-1, r);
        r
    };
    let v_done = v_minus_one;
    prefix.push(Instruction::new(Opcode::VIotaI, vec![
        OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_lane_ids),
    ]));
    prefix.push(Instruction::new(Opcode::VSplatI, vec![
        OperandType::Constant(Value::Int(1)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_lane_bound_1),
    ]));
    prefix.push(Instruction::new(Opcode::VSplatI, vec![
        OperandType::Constant(Value::Int(2)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_lane_bound_2),
    ]));

    // Re-check guards at every batch. If fewer than four candidates remain,
    // fall through to the original scalar loop at its current induction value.
    let mut fallback_branches = Vec::<usize>::new();
    // Gather executes all four physical lanes, including lanes whose virtual PC
    // is elsewhere. Guard every lookup table's required extent up front so even
    // inactive lanes (initially zero, thereafter holding a previously valid
    // index) cannot issue an out-of-bounds gather. If a bound cannot be proved,
    // required_array_length_for_index() has already rejected the transform.
    for (array, requirement) in &array_length_requirements {
        prefix.push(Instruction::new(Opcode::ArrayLen, vec![
            array.clone(), OperandType::Register(remaining),
        ]));
        let cmp = match requirement {
            ArrayLengthRequirement::AtLeastConstant(minimum) => Instruction::new(Opcode::ILtRCR, vec![
                OperandType::Register(remaining), OperandType::Constant(Value::Int(*minimum)), OperandType::Register(scalar_condition),
            ]),
            ArrayLengthRequirement::AtLeastOperand(bound) => match bound {
                OperandType::Register(_) => Instruction::new(Opcode::ILtRRR, vec![
                    OperandType::Register(remaining), bound.clone(), OperandType::Register(scalar_condition),
                ]),
                OperandType::Constant(Value::Int(_)) => Instruction::new(Opcode::ILtRCR, vec![
                    OperandType::Register(remaining), bound.clone(), OperandType::Register(scalar_condition),
                ]),
                _ => return None,
            },
        };
        prefix.push(cmp);
        let branch_idx = prefix.len();
        prefix.push(Instruction::new(Opcode::IJumpTrueR, vec![
            OperandType::Register(scalar_condition), OperandType::Offset(0),
        ]));
        fallback_branches.push(branch_idx);
    }

    let batch_head = prefix.len();
    prefix.push(Instruction::new(Opcode::IGtRCR, vec![
        OperandType::Register(induction), OperandType::Constant(Value::Int(-1)), OperandType::Register(scalar_condition),
    ]));
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(Opcode::IJumpFalseR, vec![OperandType::Register(scalar_condition), OperandType::Offset(0)]));
    let mut guarded_header = header.clone();
    let header_dst_idx = guarded_header.operands.len().checked_sub(1)?;
    guarded_header.operands[header_dst_idx] = OperandType::Register(scalar_condition);
    prefix.push(guarded_header);
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(Opcode::IJumpFalseR, vec![OperandType::Register(scalar_condition), OperandType::Offset(0)]));
    match &limit {
        OperandType::Register(limit_reg) => prefix.push(Instruction::new(Opcode::ISubRRR, vec![
            OperandType::Register(*limit_reg), OperandType::Register(induction), OperandType::Register(remaining),
        ])),
        OperandType::Constant(Value::Int(v)) => prefix.push(Instruction::new(Opcode::ISubCRR, vec![
            OperandType::Constant(Value::Int(*v)), OperandType::Register(induction), OperandType::Register(remaining),
        ])),
        _ => return None,
    }
    prefix.push(Instruction::new(Opcode::IGtRCR, vec![
        OperandType::Register(remaining), OperandType::Constant(Value::Int(SIMD_WIDTH - 1)), OperandType::Register(scalar_condition),
    ]));
    fallback_branches.push(prefix.len());
    prefix.push(Instruction::new(Opcode::IJumpFalseR, vec![OperandType::Register(scalar_condition), OperandType::Offset(0)]));

    prefix.push(Instruction::new(Opcode::VIotaI, vec![
        OperandType::Register(induction), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_induction),
    ]));
    // Initialize every lane register on every batch. Liveness ensures every
    // read-before-write is a proved Int live-in and cannot be modified in-body.
    // Emit initialization in register-number order so optimized bytecode is
    // deterministic across runs despite the lookup map's randomized iteration.
    for scalar_reg in &all_regs {
        if *scalar_reg == induction || array_base_regs.contains(scalar_reg) { continue; }
        let vector_reg = *vector_registers.get(scalar_reg)?;
        if live_ins.contains(scalar_reg) {
            prefix.push(Instruction::new(Opcode::VSplatI, vec![
                OperandType::Register(*scalar_reg), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_reg),
            ]));
        } else {
            prefix.push(Instruction::new(Opcode::VSplatI, vec![
                OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_reg),
            ]));
        }
    }
    prefix.push(Instruction::new(Opcode::VSplatI, vec![
        OperandType::Constant(Value::Int(head as i64 + 2)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_pc),
    ]));
    prefix.push(Instruction::new(Opcode::VSplatI, vec![
        OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_retmask),
    ]));
    prefix.push(Instruction::new(Opcode::VSplatI, vec![
        OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_retvalue),
    ]));

    let dispatch_head = prefix.len();
    // Leave dispatch only when every lane has reached a return or the outer
    // loop latch. -1 is impossible as a real bytecode PC.
    prefix.push(Instruction::new(Opcode::VCmpEqI, vec![
        OperandType::Register(vector_pc), OperandType::Register(v_done), OperandType::Register(vector_done_mask),
    ]));
    prefix.push(Instruction::new(Opcode::VSumI, vec![
        OperandType::Register(vector_done_mask), OperandType::Register(scalar_sum),
    ]));
    let all_done_branch = prefix.len();
    prefix.push(Instruction::new(Opcode::IEqJmpRC, vec![
        OperandType::Register(scalar_sum), OperandType::Constant(Value::Int(-4)), OperandType::Offset(0),
    ]));

    let mut skip_branches = Vec::<usize>::new();
    let mut immediate_return_branches = Vec::<usize>::new();
    let mut prune_lane_1_branches = Vec::<usize>::new();
    let mut prune_lane_2_branches = Vec::<usize>::new();

    for &pc in &region {
        let inst = &instrs[pc];
        let skip_branch = {
            let pc_as_i64 = i64::try_from(pc).ok()?;
            prefix.push(Instruction::new(Opcode::VSplatI, vec![
                OperandType::Constant(Value::Int(pc_as_i64)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_pc_const),
            ]));
            prefix.push(Instruction::new(Opcode::VCmpEqI, vec![
                OperandType::Register(vector_pc), OperandType::Register(vector_pc_const), OperandType::Register(vector_exec),
            ]));
            prefix.push(Instruction::new(Opcode::VSumI, vec![
                OperandType::Register(vector_exec), OperandType::Register(scalar_count),
            ]));
            let branch_idx = prefix.len();
            prefix.push(Instruction::new(Opcode::IEqJmpRC, vec![
                OperandType::Register(scalar_count), OperandType::Constant(Value::Int(0)), OperandType::Offset(0),
            ]));
            branch_idx
        };
        if pc == latch {
            predicated_emit_pc_update(&mut prefix, vector_exec, v_done, vector_pc);
        } else {
            match inst.opcode {
                Opcode::Nop => {
                    let next = pc.checked_add(1)?;
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int(next as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(&mut prefix, vector_exec, is_register(Some(&next_vec))?, vector_pc);
                }
                Opcode::LoadConst => {
                    let dst = is_register(inst.operands.get(1))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let source = predicated_vector_operand(
                        inst.operands.first()?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(vector_exec), source, OperandType::Register(vector_dst), OperandType::Register(vector_dst),
                    ]));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(&mut prefix, vector_exec, is_register(Some(&next_vec))?, vector_pc);
                }
                Opcode::IArrayGet => {
                    let dst = is_register(inst.operands.get(2))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let index = predicated_vector_operand(
                        inst.operands.get(1)?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let index_reg = is_register(Some(&index))?;
                    // Scalar IArrayGet clamps negative indices to zero; VGatherI
                    // does not. Normalize indices to preserve that behavior.
                    let nonnegative = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(Opcode::VCmpGtI, vec![
                        OperandType::Register(index_reg), OperandType::Register(v_minus_one), OperandType::Register(nonnegative),
                    ]));
                    let safe_index = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(nonnegative), OperandType::Register(index_reg), OperandType::Register(v_zero), OperandType::Register(safe_index),
                    ]));
                    let gathered = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(Opcode::VGatherI, vec![
                        inst.operands[0].clone(), OperandType::Register(safe_index), OperandType::Register(gathered),
                    ]));
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(vector_exec), OperandType::Register(gathered), OperandType::Register(vector_dst), OperandType::Register(vector_dst),
                    ]));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(&mut prefix, vector_exec, is_register(Some(&next_vec))?, vector_pc);
                }
                Opcode::Jump => {
                    let offset = match inst.operands.first()? { OperandType::Offset(o) => *o, _ => return None };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    let target_vec = if target == back {
                        v_done
                    } else {
                        if !region_set.contains(&target) { return None; }
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)), induction, vector_induction,
                            &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    predicated_emit_pc_update(&mut prefix, vector_exec, target_vec, vector_pc);
                }
                Opcode::JumpTrue | Opcode::JumpFalse | Opcode::IJumpTrueR | Opcode::IJumpFalseR => {
                    let offset = match inst.operands.get(1)? { OperandType::Offset(o) => *o, _ => return None };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    if target != back && !region_set.contains(&target) { return None; }
                    let condition_op = inst.operands.first()?;
                    let condition_vec = predicated_vector_operand(
                        condition_op, induction, vector_induction, &vector_registers, argument_types,
                        &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let condition_reg = is_register(Some(&condition_vec))?;
                    let condition_mask = if matches!(inst.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR) {
                        let mask = alloc_temp(&mut next_reg);
                        prefix.push(Instruction::new(Opcode::VCmpEqI, vec![
                            OperandType::Register(condition_reg), OperandType::Register(v_zero), OperandType::Register(mask),
                        ]));
                        mask
                    } else { condition_reg };
                    let target_vec = if target == back {
                        v_done
                    } else {
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)), induction, vector_induction,
                            &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    let fallthrough_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let selected_pc = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(condition_mask), OperandType::Register(target_vec), fallthrough_vec, OperandType::Register(selected_pc),
                    ]));
                    predicated_emit_pc_update(&mut prefix, vector_exec, selected_pc, vector_pc);
                }
                Opcode::IGtJmpRR | Opcode::IGtJmpRC | Opcode::ILtJmpRR | Opcode::ILtJmpRC | Opcode::IEqJmpRR | Opcode::IEqJmpRC => {
                    let offset_idx = jump_offset_index(inst)?;
                    let offset = match inst.operands.get(offset_idx)? { OperandType::Offset(o) => *o, _ => return None };
                    let target = target_from_offset(pc, offset, instrs.len())?;
                    if target != back && !region_set.contains(&target) { return None; }
                    let cmp_op = match inst.opcode {
                        Opcode::IGtJmpRR | Opcode::IGtJmpRC => Opcode::VCmpGtI,
                        Opcode::ILtJmpRR | Opcode::ILtJmpRC => Opcode::VCmpLtI,
                        Opcode::IEqJmpRR | Opcode::IEqJmpRC => Opcode::VCmpEqI,
                        _ => unreachable!(),
                    };
                    let a = predicated_vector_operand(
                        inst.operands.first()?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let b = predicated_vector_operand(
                        inst.operands.get(1)?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let mask = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(cmp_op, vec![a, b, OperandType::Register(mask)]));
                    let target_vec = if target == back {
                        v_done
                    } else {
                        let target_operand = predicated_vector_operand(
                            &OperandType::Constant(Value::Int(target as i64)), induction, vector_induction,
                            &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                        )?;
                        is_register(Some(&target_operand))?
                    };
                    let fallthrough_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let selected_pc = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(mask), OperandType::Register(target_vec), fallthrough_vec, OperandType::Register(selected_pc),
                    ]));
                    predicated_emit_pc_update(&mut prefix, vector_exec, selected_pc, vector_pc);
                }
                Opcode::Ret => {
                    let value = predicated_vector_operand(
                        inst.operands.first()?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(vector_exec), value, OperandType::Register(vector_retvalue), OperandType::Register(vector_retvalue),
                    ]));
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(vector_exec), OperandType::Register(v_minus_one), OperandType::Register(vector_retmask), OperandType::Register(vector_retmask),
                    ]));
                    predicated_emit_pc_update(&mut prefix, vector_exec, v_done, vector_pc);

                    // Preserve scalar iteration ordering: lane 0 returning wins
                    // immediately. If lane 1 or 2 returns first, freeze only
                    // higher lanes while lower lanes finish; lane 3 can return
                    // only after all lower lanes have completed.
                    for lane in 0..3usize {
                        prefix.push(Instruction::new(Opcode::VExtractI, vec![
                            OperandType::Register(vector_retmask), OperandType::Constant(Value::Int(lane as i64)), OperandType::Register(scalar_mask),
                        ]));
                        let jump_idx = prefix.len();
                        prefix.push(Instruction::new(Opcode::IEqJmpRC, vec![
                            OperandType::Register(scalar_mask), OperandType::Constant(Value::Int(-1)), OperandType::Offset(0),
                        ]));
                        if lane == 0 { immediate_return_branches.push(jump_idx); }
                        else if lane == 1 { prune_lane_1_branches.push(jump_idx); }
                        else { prune_lane_2_branches.push(jump_idx); }
                    }
                }
                _ => {
                    let vector_op = predicated_vector_opcode(inst.opcode)?;
                    let a = predicated_vector_operand(
                        inst.operands.first()?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let b = predicated_vector_operand(
                        inst.operands.get(1)?, induction, vector_induction, &vector_registers,
                        argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    let dst = is_register(inst.operands.get(2))?;
                    let vector_dst = *vector_registers.get(&dst)?;
                    let result = alloc_temp(&mut next_reg);
                    prefix.push(Instruction::new(vector_op, vec![a, b, OperandType::Register(result)]));
                    prefix.push(Instruction::new(Opcode::VSelectI, vec![
                        OperandType::Register(vector_exec), OperandType::Register(result), OperandType::Register(vector_dst), OperandType::Register(vector_dst),
                    ]));
                    let next_vec = predicated_vector_operand(
                        &OperandType::Constant(Value::Int((pc + 1) as i64)), induction, vector_induction,
                        &vector_registers, argument_types, &mut next_reg, &mut prefix, &mut const_vectors, &mut arg_vectors,
                    )?;
                    predicated_emit_pc_update(&mut prefix, vector_exec, is_register(Some(&next_vec))?, vector_pc);
                }
            }
        }
        skip_branches.push(skip_branch);
        let after_case = prefix.len();
        prefix[skip_branch].operands[2] = OperandType::Offset((after_case as i64) - (skip_branch as i64));
    }

    // After a Ret case, prune labels jump back here without falling through the
    // rest of the static instruction schedule.
    let dispatch_back = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![OperandType::Offset(dispatch_head as i64 - dispatch_back as i64)]));

    let prune_lane_1 = prefix.len();
    prefix.push(Instruction::new(Opcode::VCmpGtI, vec![
        OperandType::Register(vector_lane_ids), OperandType::Register(vector_lane_bound_1), OperandType::Register(vector_lane_mask_1),
    ]));
    prefix.push(Instruction::new(Opcode::VSelectI, vec![
        OperandType::Register(vector_lane_mask_1), OperandType::Register(v_done), OperandType::Register(vector_pc), OperandType::Register(vector_pc),
    ]));
    let jump_after_prune_1 = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![OperandType::Offset(dispatch_head as i64 - jump_after_prune_1 as i64)]));

    let prune_lane_2 = prefix.len();
    prefix.push(Instruction::new(Opcode::VCmpGtI, vec![
        OperandType::Register(vector_lane_ids), OperandType::Register(vector_lane_bound_2), OperandType::Register(vector_lane_mask_2),
    ]));
    prefix.push(Instruction::new(Opcode::VSelectI, vec![
        OperandType::Register(vector_lane_mask_2), OperandType::Register(v_done), OperandType::Register(vector_pc), OperandType::Register(vector_pc),
    ]));
    let jump_after_prune_2 = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![OperandType::Offset(dispatch_head as i64 - jump_after_prune_2 as i64)]));

    let all_done = prefix.len();
    prefix[all_done_branch].operands[2] = OperandType::Offset(all_done as i64 - all_done_branch as i64);

    // If any lane returned, select the first lane in scalar order. Otherwise,
    // process the next full batch; the scalar loop handles a 1-3 item tail.
    let mut completion_return_branches = Vec::<(usize, usize)>::new();
    for lane in 0..4usize {
        prefix.push(Instruction::new(Opcode::VExtractI, vec![
            OperandType::Register(vector_retmask), OperandType::Constant(Value::Int(lane as i64)), OperandType::Register(scalar_mask),
        ]));
        let branch_idx = prefix.len();
        prefix.push(Instruction::new(Opcode::IEqJmpRC, vec![
            OperandType::Register(scalar_mask), OperandType::Constant(Value::Int(-1)), OperandType::Offset(0),
        ]));
        completion_return_branches.push((branch_idx, lane));
    }
    prefix.push(Instruction::new(Opcode::IAddRCR, vec![
        OperandType::Register(induction), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(induction),
    ]));
    let batch_back = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![OperandType::Offset(batch_head as i64 - batch_back as i64)]));

    let mut return_labels = [0usize; 4];
    for lane in 0..4usize {
        return_labels[lane] = prefix.len();
        prefix.push(Instruction::new(Opcode::VExtractI, vec![
            OperandType::Register(vector_retvalue), OperandType::Constant(Value::Int(lane as i64)), OperandType::Register(scalar_retvalue),
        ]));
        prefix.push(Instruction::new(Opcode::Ret, vec![OperandType::Register(scalar_retvalue)]));
    }

    for (branch_idx, lane) in completion_return_branches {
        prefix[branch_idx].operands[2] = OperandType::Offset(return_labels[lane] as i64 - branch_idx as i64);
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

