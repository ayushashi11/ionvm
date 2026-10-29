//! Array aliasing, mutability, provenance, and safety proofs for vectorization.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::bytecode::instruction::{Bytecode, Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};
use crate::optimizer::direct_callee_name;
use super::analysis::*;
use super::loop_transforms::*;

// Whole-bytecode proof that an IntArray argument is immutable for the lifetime
// of every directly-visible use. The analysis intentionally rejects escaped
// functions, unknown calls, indirect values, and ambiguous register origins.
pub(super) fn op_aliases_array(op: &OperandType, alias_regs: &HashSet<usize>, alias_args: &HashSet<usize>) -> bool {
    match op {
        OperandType::Register(r) => alias_regs.contains(r),
        OperandType::Argument(a) => alias_args.contains(a),
        OperandType::RegisterList(regs) => regs.iter().any(|r| alias_regs.contains(r)),
        OperandType::PayloadKeyList(ops) => ops.iter().any(|nested| op_aliases_array(nested, alias_regs, alias_args)),
        _ => false,
    }
}

pub(super) fn is_array_read_opcode(op: Opcode) -> bool {
    matches!(op, Opcode::IArrayGet | Opcode::FArrayGet | Opcode::ArrayGet | Opcode::GetIndex | Opcode::ArrayLen)
}

pub(super) fn is_array_write_opcode(op: Opcode) -> bool {
    matches!(op, Opcode::IArraySet | Opcode::FArraySet | Opcode::ArraySet | Opcode::ArrayPush | Opcode::IBitSet | Opcode::IBitSet1 | Opcode::IBitSet0)
}

pub(super) fn local_array_argument_is_readonly(func: &Function, arg_idx: usize) -> bool {
    let alias_args = HashSet::from([arg_idx]);
    let mut alias_regs = HashSet::<usize>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for inst in &func.instructions {
            if inst.opcode != Opcode::LoadConst { continue; }
            let Some(OperandType::Register(dst)) = inst.operands.get(1) else { continue };
            let source_alias = inst.operands.first()
                .map(|op| op_aliases_array(op, &alias_regs, &alias_args)).unwrap_or(false);
            if source_alias && alias_regs.insert(*dst) { changed = true; }
        }
    }

    for inst in &func.instructions {
        let aliases: Vec<(usize, &OperandType)> = inst.operands.iter().enumerate()
            .filter(|(_, op)| op_aliases_array(op, &alias_regs, &alias_args)).collect();
        if aliases.is_empty() { continue; }
        match inst.opcode {
            Opcode::LoadConst => {
                let source_alias = inst.operands.first()
                    .map(|op| op_aliases_array(op, &alias_regs, &alias_args)).unwrap_or(false);
                let dest_alias = matches!(inst.operands.get(1), Some(OperandType::Register(r)) if alias_regs.contains(r));
                if !source_alias || !dest_alias { return false; }
            }
            op if is_array_read_opcode(op) => {
                if !inst.operands.first().map(|op| op_aliases_array(op, &alias_regs, &alias_args)).unwrap_or(false)
                    || inst.operands.iter().skip(1).any(|op| op_aliases_array(op, &alias_regs, &alias_args))
                { return false; }
            }
            Opcode::Call | Opcode::CallJit | Opcode::Spawn => {
                // This is only a local-shape check. The interprocedural proof
                // below verifies that every callee formal receiving this array
                // is itself readonly and every call target is statically known.
                if direct_callee_name_placeholder(inst).is_none() { return false; }
                let Some(OperandType::Offset(argc)) = inst.operands.get(1) else { return false };
                if *argc < 0 { return false; }
                for (operand_idx, op) in aliases {
                    if operand_idx < 2 || operand_idx >= 2 + *argc as usize || !op_aliases_array(op, &alias_regs, &alias_args) {
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
pub(super) fn direct_callee_name_placeholder(inst: &Instruction) -> Option<&str> {
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

pub(super) fn local_readonly_array_parameters(bytecode: &Bytecode) -> HashMap<String, HashSet<usize>> {
    let mut out = HashMap::new();
    for func in &bytecode.functions {
        let mut safe = HashSet::new();
        for arg in 0..func.argument_count {
            if local_array_argument_is_readonly(func, arg) { safe.insert(arg); }
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
                        if inst.opcode != Opcode::LoadConst { continue; }
                        let Some(OperandType::Register(dst)) = inst.operands.get(1) else { continue };
                        let source_alias = match inst.operands.first() {
                            Some(OperandType::Argument(a)) => *a == arg_idx,
                            Some(OperandType::Register(r)) => aliases.contains(r),
                            _ => false,
                        };
                        if source_alias && aliases.insert(*dst) { changed = true; }
                    }
                }
                let alias_args = HashSet::from([arg_idx]);
                let mut proven = true;
                for inst in &func.instructions {
                    if !matches!(inst.opcode, Opcode::Call | Opcode::CallJit | Opcode::Spawn) { continue; }
                    if !inst.operands.iter().any(|op| op_aliases_array(op, &aliases, &alias_args)) { continue; }
                    let Some(callee_name) = direct_callee_name(bytecode, inst) else { proven = false; break };
                    let Some(OperandType::Offset(argc)) = inst.operands.get(1) else { proven = false; break };
                    if *argc < 0 { proven = false; break; }
                    for idx in 0..(*argc as usize) {
                        let Some(actual) = inst.operands.get(2 + idx) else { proven = false; break };
                        if op_aliases_array(actual, &aliases, &alias_args)
                            && !out.get(&callee_name).map(|set| set.contains(&idx)).unwrap_or(false)
                        {
                            proven = false;
                            break;
                        }
                    }
                    if !proven { break; }
                }
                if !proven { remove.push((func.name.clone(), arg_idx)); }
            }
        }
        if remove.is_empty() { break; }
        for (name, arg) in remove {
            if let Some(args) = out.get_mut(&name) { args.remove(&arg); }
        }
    }
    out
}


pub(super) fn unique_int_array_constant(bytecode: &Bytecode, wanted: &std::sync::Arc<std::sync::RwLock<Vec<i64>>>) -> bool {
    let mut count = 0usize;
    for func in &bytecode.functions {
        for inst in &func.instructions {
            for op in &inst.operands {
                if let OperandType::Constant(Value::IntArray(arr)) = op {
                    if std::sync::Arc::ptr_eq(arr, wanted) { count += 1; }
                }
            }
        }
    }
    count == 1
}

pub(super) fn array_argument_origins_are_safe(
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
        let Some(func) = bytecode.functions.get(function_idx) else { return false };
        if function_idx == 0 || func.name == "main" || escaped.contains(&func.name)
            || caller_arg_types.get(&func.name).and_then(|a| a.get(arg_idx)).copied() != Some(ValType::IntArray)
            || !local_readonly.get(&func.name).map(|s| s.contains(&arg_idx)).unwrap_or(false)
            || !visiting.insert((function_idx, arg_idx))
        { return false; }

        let mut saw_site = false;
        for (caller_idx, caller) in bytecode.functions.iter().enumerate() {
            let caller_args = caller_arg_types.get(&caller.name).cloned().unwrap_or_else(|| unknown_argument_types(caller));
            let caller_analysis = analyze_types(caller, returns, &caller_args);
            for (pc, inst) in caller.instructions.iter().enumerate() {
                if direct_callee_name(bytecode, inst).as_deref() != Some(func.name.as_str()) { continue; }
                saw_site = true;
                let Some(actual) = inst.operands.get(2 + arg_idx) else {
                    visiting.remove(&(function_idx, arg_idx));
                    return false;
                };
                let actual_type = caller_analysis.before.get(pc).and_then(|s| s.as_ref())
                    .map(|state| operand_type_with_args(actual, state, &caller_args)).unwrap_or(ValType::Unknown);
                if actual_type != ValType::IntArray || !actual_origin_safe(
                    bytecode, returns, caller_arg_types, caller_idx, actual, pc,
                    local_readonly, escaped, visiting,
                ) {
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
        let Some(caller) = bytecode.functions.get(caller_idx) else { return false };
        match actual {
            OperandType::Argument(a) => formal_arg_safe(
                bytecode, returns, argument_types, caller_idx, *a, local_readonly, escaped, visiting,
            ),
            OperandType::Constant(Value::IntArray(arr)) => unique_int_array_constant(bytecode, arr),
            OperandType::Register(r) => {
                let defs: Vec<(usize, &Instruction)> = caller.instructions.iter().enumerate()
                    .filter(|(_, inst)| instruction_written_registers(inst).contains(r)).collect();
                if defs.len() != 1 { return false; }
                let (def_pc, def) = defs[0];
                if def_pc >= call_pc || !instruction_dominates_use(caller, def_pc, call_pc) { return false; }
                match def.opcode {
                    Opcode::NewIntArray => {
                        let dest_idx = if def.operands.len() >= 2 { 1 } else { 0 };
                        if is_register(def.operands.get(dest_idx)) != Some(*r) { return false; }
                        local_array_lifetime_is_safe(
                            bytecode, returns, argument_types, caller_idx, *r, def_pc,
                            local_readonly, escaped, visiting,
                        )
                    }
                    Opcode::LoadConst if is_register(def.operands.get(1)) == Some(*r) => {
                        let Some(source) = def.operands.first() else { return false };
                        match source {
                            OperandType::Register(src) => actual_origin_safe(
                                bytecode, returns, argument_types, caller_idx, &OperandType::Register(*src), def_pc,
                                local_readonly, escaped, visiting,
                            ),
                            OperandType::Argument(a) => formal_arg_safe(
                                bytecode, returns, argument_types, caller_idx, *a, local_readonly, escaped, visiting,
                            ),
                            OperandType::Constant(Value::IntArray(arr)) if unique_int_array_constant(bytecode, arr) => {
                                local_array_lifetime_is_safe(
                                    bytecode, returns, argument_types, caller_idx, *r, def_pc,
                                    local_readonly, escaped, visiting,
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
        let Some(caller) = bytecode.functions.get(caller_idx) else { return false };
        let mut aliases = HashSet::from([root_reg]);
        let mut changed = true;
        while changed {
            changed = false;
            for (pc, inst) in caller.instructions.iter().enumerate().skip(origin_pc + 1) {
                if inst.opcode != Opcode::LoadConst { continue; }
                let Some(OperandType::Register(dst)) = inst.operands.get(1) else { continue };
                if matches!(inst.operands.first(), Some(OperandType::Register(src)) if aliases.contains(src))
                    && aliases.insert(*dst) { changed = true; }
                let _ = pc;
            }
        }
        // Every alias must have exactly one definition, and that definition
        // must dominate every read. Otherwise a register reuse or a branch that
        // bypasses initialization could make the runtime value unrelated.
        for alias in &aliases {
            let defs: Vec<(usize, &Instruction)> = caller.instructions.iter().enumerate()
                .filter(|(_, inst)| instruction_written_registers(inst).contains(alias)).collect();
            if defs.len() != 1 { return false; }
            let (def_pc, def) = defs[0];
            let valid_origin = def_pc == origin_pc && *alias == root_reg && (
                (def.opcode == Opcode::NewIntArray
                    && is_register(def.operands.get(if def.operands.len() >= 2 { 1 } else { 0 })) == Some(*alias))
                || (def.opcode == Opcode::LoadConst
                    && is_register(def.operands.get(1)) == Some(*alias)
                    && matches!(def.operands.first(), Some(OperandType::Constant(Value::IntArray(_)))) )
            );
            let valid_copy = def_pc > origin_pc && def.opcode == Opcode::LoadConst
                && is_register(def.operands.get(1)) == Some(*alias)
                && matches!(def.operands.first(), Some(OperandType::Register(src)) if aliases.contains(src));
            if !valid_origin && !valid_copy { return false; }
            for (use_pc, inst) in caller.instructions.iter().enumerate() {
                if use_pc == def_pc || !instruction_reads(inst).contains(alias) { continue; }
                if !instruction_dominates_use(caller, def_pc, use_pc) { return false; }
            }
        }

        let mut first_spawn: Option<usize> = None;
        for (pc, inst) in caller.instructions.iter().enumerate() {
            if inst.opcode != Opcode::Spawn { continue; }
            let Some(callee_name) = direct_callee_name(bytecode, inst) else { continue };
            let Some(OperandType::Offset(argc)) = inst.operands.get(1) else { return false };
            if *argc < 0 { return false; }
            for arg_idx in 0..(*argc as usize) {
                let Some(op) = inst.operands.get(2 + arg_idx) else { return false };
                if op_aliases_array(op, &aliases, &HashSet::new()) {
                    if !local_readonly.get(&callee_name).map(|s| s.contains(&arg_idx)).unwrap_or(false) {
                        return false;
                    }
                    first_spawn = Some(first_spawn.map_or(pc, |old| old.min(pc)));
                }
            }
        }

        for (pc, inst) in caller.instructions.iter().enumerate() {
            let uses_alias = inst.operands.iter().any(|op| op_aliases_array(op, &aliases, &HashSet::new()));
            let is_origin = pc == origin_pc && (
                (inst.opcode == Opcode::NewIntArray && instruction_written_registers(inst).contains(&root_reg))
                || (inst.opcode == Opcode::LoadConst
                    && is_register(inst.operands.get(1)) == Some(root_reg)
                    && matches!(inst.operands.first(), Some(OperandType::Constant(Value::IntArray(_)))) )
            );
            if is_origin { continue; }
            if !uses_alias { continue; }
            match inst.opcode {
                Opcode::LoadConst => {
                    let source_alias = inst.operands.first()
                        .map(|op| op_aliases_array(op, &aliases, &HashSet::new())).unwrap_or(false);
                    let dest_alias = matches!(inst.operands.get(1), Some(OperandType::Register(r)) if aliases.contains(r));
                    if pc <= origin_pc || !source_alias || !dest_alias { return false; }
                }
                op if is_array_read_opcode(op) => {
                    if !inst.operands.first().map(|op| op_aliases_array(op, &aliases, &HashSet::new())).unwrap_or(false)
                        || inst.operands.iter().skip(1).any(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                    { return false; }
                }
                op if is_array_write_opcode(op) => {
                    if !inst.operands.first().map(|op| op_aliases_array(op, &aliases, &HashSet::new())).unwrap_or(false)
                        || inst.operands.iter().skip(1).any(|op| op_aliases_array(op, &aliases, &HashSet::new()))
                        || first_spawn.map(|shared_pc| pc >= shared_pc).unwrap_or(false)
                    { return false; }
                }
                Opcode::Call | Opcode::CallJit | Opcode::Spawn => {
                    let Some(callee_name) = direct_callee_name(bytecode, inst) else { return false };
                    let Some(OperandType::Offset(argc)) = inst.operands.get(1) else { return false };
                    if *argc < 0 { return false; }
                    let Some(arg_end) = 2usize.checked_add(*argc as usize) else { return false; };
                    // Every occurrence of this array alias in the instruction
                    // must be an actual argument. It may not be a call target,
                    // arity/table metadata, or a destination register.
                    for (operand_idx, op) in inst.operands.iter().enumerate() {
                        if op_aliases_array(op, &aliases, &HashSet::new())
                            && (operand_idx < 2 || operand_idx >= arg_end)
                        { return false; }
                    }
                    for arg_idx in 0..(*argc as usize) {
                        let Some(op) = inst.operands.get(2 + arg_idx) else { return false };
                        if op_aliases_array(op, &aliases, &HashSet::new())
                            && !local_readonly.get(&callee_name).map(|args| args.contains(&arg_idx)).unwrap_or(false)
                        { return false; }
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
        let args = argument_types.get(&func.name).cloned().unwrap_or_else(|| unknown_argument_types(func));
        let Some(function_idx) = function_index(bytecode, &func.name) else { continue };
        let mut safe_args = HashSet::new();
        for arg_idx in 0..func.argument_count {
            if args.get(arg_idx).copied() != Some(ValType::IntArray) { continue; }
            let mut visiting = HashSet::new();
            if formal_arg_safe(bytecode, returns, argument_types, function_idx, arg_idx,
                local_readonly, &escaped, &mut visiting) {
                safe_args.insert(arg_idx);
            }
        }
        safe.insert(func.name.clone(), safe_args);
    }
    safe
}


