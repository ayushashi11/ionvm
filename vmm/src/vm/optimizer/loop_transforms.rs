//! Scalar loop transforms, the canonical array-loop SIMD prefix, and PC relocation.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::bytecode::instruction::{Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};

use super::analysis::*;
use super::SIMD_WIDTH;

pub(super) fn array_base_register(inst: &Instruction) -> Option<usize> {
    match inst.operands.first() {
        Some(OperandType::Register(r)) => Some(*r),
        _ => None,
    }
}

pub(super) fn is_private_array_register(func: &Function, arr: usize, head: usize, back: usize) -> bool {
    // Require a local NewIntArray definition before the loop. This deliberately
    // excludes argument arrays and copied/shared references: the current VM has
    // no escape/alias metadata with which to prove those cases safe to reorder.
    let mut origin = None;
    for pc in 0..head {
        let inst = &func.instructions[pc];
        if instruction_written_registers(inst).contains(&arr) {
            origin = if inst.opcode == Opcode::NewIntArray
                && is_register(inst.operands.get(if inst.operands.len() >= 2 { 1 } else { 0 })) == Some(arr)
            {
                Some(pc)
            } else {
                None
            };
        }
    }
    let Some(origin_pc) = origin else { return false };

    // Closure captures use a separate operand representation; the register-use
    // walker below cannot prove that a capture does not alias this array.
    if func.instructions.iter().enumerate().any(|(pc, inst)| {
        (origin_pc..=back).contains(&pc) && inst.opcode == Opcode::MakeClosure
    }) {
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
            Opcode::ArrayGet | Opcode::GetIndex | Opcode::IArrayGet | Opcode::ArraySet
                | Opcode::IArraySet | Opcode::ArrayLen | Opcode::ArrayPush
        ) && is_register(inst.operands.first()) == Some(arr)
            && !inst.operands.iter().enumerate().any(|(idx, op)| idx != 0 && operand_contains_register(op, arr));
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
pub(super) struct StorePlan {
    array: OperandType,
    value: StoreValue,
}

pub(super) fn array_argument_origin_before(func: &Function, reg: usize, use_pc: usize) -> Option<usize> {
    fn recurse(func: &Function, reg: usize, use_pc: usize, seen: &mut HashSet<usize>) -> Option<usize> {
        if !seen.insert(reg) { return None; }
        let defs: Vec<(usize, &Instruction)> = func.instructions.iter().enumerate()
            .filter(|(_, inst)| instruction_written_registers(inst).contains(&reg)).collect();
        if defs.len() != 1 { return None; }
        let (def_pc, inst) = defs[0];
        if def_pc >= use_pc || !instruction_dominates_use(func, def_pc, use_pc) || inst.opcode != Opcode::LoadConst
            || is_register(inst.operands.get(1)) != Some(reg) { return None; }
        match inst.operands.first()? {
            OperandType::Argument(arg) => Some(*arg),
            OperandType::Register(src) => recurse(func, *src, def_pc, seen),
            _ => None,
        }
    }
    recurse(func, reg, use_pc, &mut HashSet::new())
}

pub(super) fn build_int_simd_prefix(
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
    if !matches!(head_inst.opcode, Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR)
        || head_inst.operands.len() < 3
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
        OperandType::Register(r) if head_state.get(*r).copied() != Some(ValType::Int) => return None,
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
                    || !matches!(inst.operands.first(), Some(OperandType::Constant(Value::Int(_))))
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
        if matches!(inst.opcode, Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR
            | Opcode::IAddRCR | Opcode::ISubRCR | Opcode::ISubCRR | Opcode::IMulRCR)
        {
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
            if dst == ind || dst == cond || matches!(&limit, OperandType::Register(r) if *r == dst) {
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
        if instrs.iter().enumerate().any(|(pc, inst)| pc > back && instruction_mentions_register(inst, *reg)) {
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
            if *source != ind && body[store_idx + 1..].iter().any(|later| {
                instruction_written_registers(later).contains(source)
            }) {
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
    if body.iter().any(|inst| instruction_written_registers(inst).contains(&ind)) {
        return None;
    }
    if let OperandType::Register(limit_reg) = &limit {
        if body.iter().any(|inst| instruction_written_registers(inst).contains(limit_reg)) {
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
    prefix.push(Instruction::new(Opcode::ILtRCR, vec![
        OperandType::Register(ind), OperandType::Constant(Value::Int(0)), OperandType::Register(r_guard),
    ]));
    let guard_negative_branch = prefix.len();
    prefix.push(Instruction::new(Opcode::IJumpTrueR, vec![
        OperandType::Register(r_guard), OperandType::Offset(0),
    ]));

    // A runtime negative bound also falls back to the original loop. It may
    // still execute iterations if the starting index is even more negative.
    if let OperandType::Register(limit_reg) = &limit {
        prefix.push(Instruction::new(Opcode::ILtRCR, vec![
            OperandType::Register(*limit_reg), OperandType::Constant(Value::Int(0)), OperandType::Register(r_guard),
        ]));
        prefix.push(Instruction::new(Opcode::IJumpTrueR, vec![
            OperandType::Register(r_guard), OperandType::Offset(0),
        ]));
    }

    // Any array access beyond a typed-array length would have different timing or
    // failure behavior if gathered in a vector. Preflight every unique array; if
    // the requested limit exceeds any length, run the original scalar loop.
    for arr in &arrays {
        prefix.push(Instruction::new(Opcode::ArrayLen, vec![
            OperandType::Register(*arr), OperandType::Register(r_array_len),
        ]));
        let limit_check = match &limit {
            OperandType::Register(limit_reg) => Instruction::new(Opcode::ILtRRR, vec![
                OperandType::Register(r_array_len), OperandType::Register(*limit_reg), OperandType::Register(r_guard),
            ]),
            OperandType::Constant(Value::Int(v)) => Instruction::new(Opcode::ILtRCR, vec![
                OperandType::Register(r_array_len), OperandType::Constant(Value::Int(*v)), OperandType::Register(r_guard),
            ]),
            _ => return None,
        };
        prefix.push(limit_check);
        prefix.push(Instruction::new(Opcode::IJumpTrueR, vec![
            OperandType::Register(r_guard), OperandType::Offset(0),
        ]));
        // Patched after the total prefix length is known.
    }

    let vector_head = prefix.len();
    // Avoid `ind + 3 < limit`: after a final full batch, `ind` may become
    // i64::MAX and the addition would wrap on the next header. With non-negative
    // ind/limit, `remaining = limit - ind` stays within [-i64::MAX, i64::MAX].
    match &limit {
        OperandType::Register(limit_reg) => prefix.push(Instruction::new(Opcode::ISubRRR, vec![
            OperandType::Register(*limit_reg), OperandType::Register(ind), OperandType::Register(r_remaining),
        ])),
        OperandType::Constant(Value::Int(v)) => prefix.push(Instruction::new(Opcode::ISubCRR, vec![
            OperandType::Constant(Value::Int(*v)), OperandType::Register(ind), OperandType::Register(r_remaining),
        ])),
        _ => return None,
    }
    prefix.push(Instruction::new(Opcode::IGtRCR, vec![
        OperandType::Register(r_remaining), OperandType::Constant(Value::Int(SIMD_WIDTH - 1)), OperandType::Register(r_vec_cond),
    ]));
    let vector_exit_branch = prefix.len();
    prefix.push(Instruction::new(Opcode::IJumpFalseR, vec![
        OperandType::Register(r_vec_cond), OperandType::Offset(0),
    ]));
    prefix.push(Instruction::new(Opcode::VIotaI, vec![
        OperandType::Register(ind), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(r_vec_index),
    ]));
    for (lane, lane_reg) in [(1i64, lane1), (2, lane2), (3, lane3)] {
        prefix.push(Instruction::new(Opcode::VExtractI, vec![
            OperandType::Register(r_vec_index), OperandType::Constant(Value::Int(lane)), OperandType::Register(lane_reg),
        ]));
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
                    emitted.push(Instruction::new(Opcode::VSplatI, vec![
                        OperandType::Register(*r), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(v),
                    ]));
                    scalar_splats.insert(*r, v);
                    v
                };
                Some(OperandType::Register(v))
            }
            OperandType::Constant(Value::Int(value)) => {
                let v = alloc_temp(next_reg);
                emitted.push(Instruction::new(Opcode::VSplatI, vec![
                    OperandType::Constant(Value::Int(*value)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(v),
                ]));
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
                vector_body.push(Instruction::new(Opcode::VSplatI, vec![
                    source, OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_dst),
                ]));
            }
            Opcode::IArrayGet => {
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                vector_body.push(Instruction::new(Opcode::VGatherI, vec![
                    inst.operands[0].clone(), OperandType::Register(r_vec_index), OperandType::Register(vector_dst),
                ]));
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
                store_plans.push(StorePlan { array: inst.operands[0].clone(), value: plan_value });
            }
            Opcode::IAddRRR | Opcode::ISubRRR | Opcode::IMulRRR => {
                let a = ensure_vector_operand(&inst.operands[0], state, ind, r_vec_index, &vector_registers, &mut scalar_splats, &mut next_reg, &mut vector_body)?;
                let b = ensure_vector_operand(&inst.operands[1], state, ind, r_vec_index, &vector_registers, &mut scalar_splats, &mut next_reg, &mut vector_body)?;
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                let opcode = match inst.opcode {
                    Opcode::IAddRRR => Opcode::VAddI,
                    Opcode::ISubRRR => Opcode::VSubI,
                    Opcode::IMulRRR => Opcode::VMulI,
                    _ => unreachable!(),
                };
                vector_body.push(Instruction::new(opcode, vec![a, b, OperandType::Register(vector_dst)]));
            }
            Opcode::IAddRCR | Opcode::ISubRCR | Opcode::IMulRCR => {
                let a = ensure_vector_operand(&inst.operands[0], state, ind, r_vec_index, &vector_registers, &mut scalar_splats, &mut next_reg, &mut vector_body)?;
                let k = constant_int(inst.operands.get(1)?)?;
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                let opcode = match inst.opcode {
                    Opcode::IAddRCR => Opcode::VAddIC,
                    Opcode::ISubRCR => Opcode::VSubIC,
                    Opcode::IMulRCR => Opcode::VMulIC,
                    _ => unreachable!(),
                };
                vector_body.push(Instruction::new(opcode, vec![a, OperandType::Constant(Value::Int(k)), OperandType::Register(vector_dst)]));
            }
            Opcode::ISubCRR => {
                let k = constant_int(inst.operands.first()?)?;
                let a = ensure_vector_operand(&inst.operands[1], state, ind, r_vec_index, &vector_registers, &mut scalar_splats, &mut next_reg, &mut vector_body)?;
                let constant_vector = alloc_temp(&mut next_reg);
                vector_body.push(Instruction::new(Opcode::VSplatI, vec![
                    OperandType::Constant(Value::Int(k)), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(constant_vector),
                ]));
                let dst = is_register(inst.operands.get(2))?;
                let vector_dst = *vector_registers.get(&dst)?;
                vector_body.push(Instruction::new(Opcode::VSubI, vec![
                    OperandType::Register(constant_vector), a, OperandType::Register(vector_dst),
                ]));
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
                    prefix.push(Instruction::new(Opcode::VExtractI, vec![
                        OperandType::Register(*vector_reg), OperandType::Constant(Value::Int(lane as i64)), OperandType::Register(r_scalar_extract),
                    ]));
                    OperandType::Register(r_scalar_extract)
                }
            };
            prefix.push(Instruction::new(Opcode::IArraySet, vec![
                store.array.clone(), OperandType::Register(lane_indices[lane]), scalar_value,
            ]));
        }
    }

    // Advance by four and branch back to the vector guard.
    prefix.push(Instruction::new(Opcode::IAddRCR, vec![
        OperandType::Register(ind), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(ind),
    ]));
    let vector_back = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![OperandType::Offset(vector_head as i64 - vector_back as i64)]));

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

pub(super) fn offset_is_non_branch_metadata(opcode: Opcode, idx: usize) -> bool {
    match opcode {
        Opcode::Call | Opcode::Spawn => idx == 1, // argument count
        Opcode::CallJit => idx == 0 || idx == 1,  // function-table index, argument count
        _ => false,
    }
}

pub(super) fn all_offsets_are_relocatable(func: &Function) -> bool {
    for inst in &func.instructions {
        for (idx, op) in inst.operands.iter().enumerate() {
            match op {
                OperandType::Offset(_) => {
                    if !offset_is_non_branch_metadata(inst.opcode, idx) && jump_offset_index(inst) != Some(idx) {
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

pub(super) fn remap_pc(old_pc: usize, insertion_pc: usize, amount: usize) -> usize {
    if old_pc >= insertion_pc { old_pc.saturating_add(amount) } else { old_pc }
}

pub(super) fn relocate_instruction_offsets(inst: &mut Instruction, old_pc: usize, new_pc: usize, insertion_pc: usize, amount: usize, old_len: usize) -> bool {
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
            let Some(old_target) = target_from_offset(old_pc, *off, old_len) else { return false };
            let new_target = remap_pc(old_target, insertion_pc, amount);
            *off = new_target as i64 - new_pc as i64;
        }
        if let OperandType::JumpTable(cases) = op {
            if inst.opcode != Opcode::ETagSwitch {
                return false;
            }
            for case in cases.iter_mut() {
                let Some(old_target) = target_from_offset(old_pc, case.offset, old_len) else { return false };
                let new_target = remap_pc(old_target, insertion_pc, amount);
                case.offset = new_target as i64 - new_pc as i64;
            }
        }
    }
    true
}

pub(super) fn insert_prefix_with_relocation(func: &mut Function, insertion_pc: usize, prefix: Vec<Instruction>, new_register_count: usize) -> bool {
    if prefix.is_empty() || insertion_pc > func.instructions.len() || !all_offsets_are_relocatable(func) {
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
pub(super) fn remove_redundant_copy_roundtrips(func: &mut Function) -> bool {
    let len = func.instructions.len();
    if len < 3 { return false; }
    let mut incoming = HashMap::<usize, Vec<usize>>::new();
    for (pc, inst) in func.instructions.iter().enumerate() {
        if let Some(target) = direct_branch_target(inst, pc, len) {
            incoming.entry(target).or_default().push(pc);
        }
        for op in &inst.operands {
            if let OperandType::JumpTable(cases) = op {
                if inst.opcode != Opcode::ETagSwitch { continue; }
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
        if a.opcode != Opcode::LoadConst || b.opcode != Opcode::Jump || c.opcode != Opcode::LoadConst {
            continue;
        }
        let (Some(OperandType::Register(src)), Some(OperandType::Register(tmp))) = (a.operands.first(), a.operands.get(1)) else { continue };
        if src == tmp || is_register(c.operands.first()) != Some(*tmp) || is_register(c.operands.get(1)) != Some(*src) {
            continue;
        }
        if !matches!(b.operands.first(), Some(OperandType::Offset(1))) { continue; }
        // An external branch into either middle instruction would bypass part
        // of the copy sequence and can make the apparent round-trip observable.
        if incoming.get(&(pc + 1)).map(|sources| !sources.is_empty()).unwrap_or(false) {
            continue;
        }
        if incoming.get(&(pc + 2)).map(|sources| sources.iter().any(|source| *source != pc + 1)).unwrap_or(false) {
            continue;
        }
        // The temp may only be referenced by the two copies.
        let outside_temp_use = func.instructions.iter().enumerate().any(|(other_pc, inst)| {
            if (pc..=pc + 2).contains(&other_pc) { return false; }
            instruction_mentions_register(inst, *tmp)
        });
        if outside_temp_use { continue; }
        func.instructions[pc] = Instruction::new(Opcode::Nop, vec![]);
        func.instructions[pc + 1] = Instruction::new(Opcode::Nop, vec![]);
        func.instructions[pc + 2] = Instruction::new(Opcode::Nop, vec![]);
        changed = true;
    }
    changed
}

pub(super) const MAX_UNROLLED_TRIPS: i64 = 4;
pub(super) const MAX_UNROLLED_BODY: usize = 32;
pub(super) const MAX_UNROLL_ROUNDS: usize = 32;

pub(super) fn is_pure_unroll_opcode(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Nop
            | Opcode::LoadConst
            | Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Rem
            | Opcode::Eq | Opcode::Lt | Opcode::Gt | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Neg | Opcode::Not
            | Opcode::IAddRRR | Opcode::IAddRCR
            | Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR
            | Opcode::IMulRRR | Opcode::IMulRCR
            | Opcode::IDivRCR | Opcode::IDivCRR | Opcode::IRemRCR | Opcode::IRemCRR
            | Opcode::IEqRRR | Opcode::IEqRCR
            | Opcode::ILtRRR | Opcode::ILtRCR | Opcode::ILtCRR
            | Opcode::IGtRRR | Opcode::IGtRCR | Opcode::IGtCRR
            | Opcode::FAddRRR | Opcode::FAddRCR
            | Opcode::FSubRRR | Opcode::FSubRCR | Opcode::FSubCRR
            | Opcode::FMulRRR | Opcode::FMulRCR
            | Opcode::FDivRRR | Opcode::FDivRCR | Opcode::FDivCRR
            | Opcode::FEqRRR | Opcode::FEqRCR
            | Opcode::FLtRRR | Opcode::FLtRCR | Opcode::FLtCRR
            | Opcode::FGtRRR | Opcode::FGtRCR | Opcode::FGtCRR
            | Opcode::SEqRRR | Opcode::SConcatRRR
            | Opcode::IAndRRR | Opcode::IAndRCR
            | Opcode::IOrRRR | Opcode::IOrRCR
            | Opcode::IXorRRR | Opcode::IXorRCR
            | Opcode::IShlRRR | Opcode::IShlRCR
            | Opcode::IShrRRR | Opcode::IShrRCR
            | Opcode::Jump | Opcode::JumpTrue | Opcode::JumpFalse
            | Opcode::IJumpTrueR | Opcode::IJumpFalseR
            | Opcode::IGtJmpRR | Opcode::IGtJmpRC
            | Opcode::ILtJmpRR | Opcode::ILtJmpRC
            | Opcode::IEqJmpRR | Opcode::IEqJmpRC
            | Opcode::FGtJmpRR | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR | Opcode::FLtJmpRC
            | Opcode::ETagJmpRC
            | Opcode::Ret
    )
}

pub(super) fn direct_branch_target(inst: &Instruction, pc: usize, len: usize) -> Option<usize> {
    let idx = jump_offset_index(inst)?;
    let OperandType::Offset(off) = inst.operands.get(idx)? else { return None };
    target_from_offset(pc, *off, len)
}

pub(super) fn has_external_entry_into_loop(func: &Function, head: usize, back: usize) -> bool {
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
                    let Some(target) = target_from_offset(pc, case.offset, len) else { return true };
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
pub(super) fn build_small_unroll(
    func: &Function,
    head: usize,
    back: usize,
) -> Option<Vec<Instruction>> {
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
    if !matches!(condition_branch.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR)
        || is_register(condition_branch.operands.first()) != Some(cond)
    {
        return None;
    }
    let exit = direct_branch_target(condition_branch, head + 1, func.instructions.len())?;
    if exit != back + 1 {
        return None;
    }
    let back_inst = func.instructions.get(back)?;
    if back_inst.opcode != Opcode::Jump || direct_branch_target(back_inst, back, func.instructions.len())? != head {
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
    if func.instructions.iter().enumerate().any(|(pc, inst)| {
        pc > back && instruction_mentions_register(inst, cond)
    }) {
        return None;
    }

    let body_start = head + 2;
    let body_end = back; // Exclusive; includes the induction increment at back - 1.
    let body = &func.instructions[body_start..body_end];
    if body.is_empty() || body.len().checked_mul(trips)? > MAX_UNROLLED_BODY * MAX_UNROLLED_TRIPS as usize {
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
        if inst.operands.iter().any(|op| matches!(op, OperandType::JumpTable(_))) {
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
                let OperandType::Offset(off) = inst.operands.get(branch_idx)? else { return None };
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

pub(super) fn remap_target_for_replacement(
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

pub(super) fn relocate_outside_for_replacement(
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
            let Some(target) = target_from_offset(old_pc, *off, old_len) else { return false };
            let Some(new_target) = remap_target_for_replacement(target, start, end_inclusive, replacement_len) else { return false };
            *off = new_target as i64 - new_pc as i64;
        }
        if let OperandType::JumpTable(cases) = op {
            if inst.opcode != Opcode::ETagSwitch {
                return false;
            }
            for case in cases.iter_mut() {
                let Some(target) = target_from_offset(old_pc, case.offset, old_len) else { return false };
                let Some(new_target) = remap_target_for_replacement(target, start, end_inclusive, replacement_len) else { return false };
                case.offset = new_target as i64 - new_pc as i64;
            }
        }
    }
    true
}

pub(super) fn replace_loop_with_unrolled_body(
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
            &mut inst, old_pc, old_pc, head, back, replacement_len, old_len,
        ) {
            return false;
        }
        new_instructions.push(inst);
    }
    new_instructions.extend(replacement);
    for old_pc in back + 1..old_len {
        let mut inst = func.instructions[old_pc].clone();
        let Some(new_pc) = remap_target_for_replacement(old_pc, head, back, replacement_len) else { return false };
        if !relocate_outside_for_replacement(
            &mut inst, old_pc, new_pc, head, back, replacement_len, old_len,
        ) {
            return false;
        }
        new_instructions.push(inst);
    }
    func.instructions = new_instructions;
    true
}

pub(super) fn unroll_small_constant_loops(func: &mut Function) -> bool {
    let mut changed = false;
    for _ in 0..MAX_UNROLL_ROUNDS {
        let mut transformed = false;
        let len = func.instructions.len();
        for back in 1..len {
            let Some(OperandType::Offset(off)) = func.instructions[back].operands.first() else { continue };
            if func.instructions[back].opcode != Opcode::Jump || *off >= 0 {
                continue;
            }
            let Some(head) = target_from_offset(back, *off, len) else { continue };
            if head >= back {
                continue;
            }
            let Some(replacement) = build_small_unroll(func, head, back) else { continue };
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

