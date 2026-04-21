use super::super::{ExecutionResult, IonVM};
use crate::value::process::{Process, ProcessStatus};
use crate::value::{Primitive, Value};
use crate::vm::timeout::TimeoutInfo;

use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn exec_spawn(
    vm: &mut IonVM,
    proc: &mut Process,
    dst_reg: usize,
    func_reg: usize,
    arg_regs: Vec<usize>,
) -> ExecutionResult {
    let frame = proc.frames.last().expect("active frame required");
    let func_val = frame.registers[func_reg].clone();
    let args: Vec<Value> = arg_regs
        .iter()
        .map(|&r| frame.registers[r].clone())
        .collect();

    match func_val {
        Value::Function(func_rc) => {
            let prefix = {
                let func = func_rc.borrow();
                if let Some(env_rc) = &func.closure_env {
                    let env = env_rc.borrow();
                    func.capture_order
                        .iter()
                        .map(|name| {
                            env.get(name)
                                .cloned()
                                .unwrap_or(Value::Primitive(Primitive::Undefined))
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            };

            let mut spawn_args = prefix;
            spawn_args.extend(args);
            let new_pid = vm.spawn_process(func_rc, spawn_args);
            if let Some(new_proc) = vm.processes.get(&new_pid) {
                proc.frames.last_mut().unwrap().registers[dst_reg] =
                    Value::Process(new_proc.clone());
            }
        }
        _ => {
            eprintln!("[VM] spawn: target register does not contain a function");
            proc.frames.last_mut().unwrap().registers[dst_reg] =
                Value::Primitive(Primitive::Undefined);
        }
    }
    ExecutionResult::Continue
}

pub(super) fn exec_send(
    vm: &mut IonVM,
    proc: &mut Process,
    proc_reg: usize,
    msg_reg: usize,
) -> ExecutionResult {
    let frame = proc.frames.last().expect("active frame required");
    let target = frame.registers[proc_reg].clone();
    let msg = frame.registers[msg_reg].clone();

    if let Value::Process(target_rc) = target {
        let target_pid = target_rc.borrow().pid;
        let target_status = target_rc.borrow().status.clone();

        if target_status == ProcessStatus::WaitingForMessage {
            // Check if the target was blocked on ReceiveWithTimeout
            if let Some((dst, result)) = vm.take_pending_timeout(target_pid) {
                // Deliver directly to the waiting registers instead of going through the mailbox.
                // The process continues from the instruction after ReceiveWithTimeout on wake.
                let mut target = target_rc.borrow_mut();
                if let Some(frame) = target.frames.last_mut() {
                    frame.registers[dst] = msg;
                    frame.registers[result] = Value::Primitive(Primitive::Boolean(true));
                }
                drop(target);
            } else {
                // Regular Receive: put in mailbox, the instruction will consume it on re-run
                target_rc.borrow_mut().mailbox.push_back(msg);
            }
            vm.wake_process(target_pid);
        } else {
            // Process is not blocking — queue the message
            target_rc.borrow_mut().mailbox.push_back(msg);
        }
    }
    ExecutionResult::Continue
}

pub(super) fn exec_receive(proc: &mut Process, dst: usize) -> ExecutionResult {
    if let Some(msg) = proc.mailbox.pop_front() {
        proc.frames
            .last_mut()
            .expect("active frame required")
            .registers[dst] = msg;
        ExecutionResult::Continue
    } else {
        // Block; execute_process_slice will revert IP so this instruction retries on wake
        ExecutionResult::Blocked
    }
}

pub(super) fn exec_receive_with_timeout(
    vm: &mut IonVM,
    proc: &mut Process,
    dst: usize,
    timeout_reg: usize,
    result_reg: usize,
) -> ExecutionResult {
    if let Some(msg) = proc.mailbox.pop_front() {
        // Message is already here — cancel any stale timeout and deliver
        vm.pending_timeouts.retain(|t| t.pid != proc.pid);
        let frame = proc.frames.last_mut().expect("active frame required");
        frame.registers[dst] = msg;
        frame.registers[result_reg] = Value::Primitive(Primitive::Boolean(true));
        return ExecutionResult::Continue;
    }

    // No message — set default "timed out" values upfront, then register the timeout
    let timeout_ms = {
        let frame = proc.frames.last().expect("active frame required");
        match &frame.registers[timeout_reg] {
            Value::Primitive(Primitive::Number(n)) => *n as u64,
            _ => return ExecutionResult::Error("timeout register must hold a number".to_string()),
        }
    };

    {
        let frame = proc.frames.last_mut().unwrap();
        frame.registers[dst] = Value::Primitive(Primitive::Undefined);
        frame.registers[result_reg] = Value::Primitive(Primitive::Boolean(false));
    }

    let expiry_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        + timeout_ms;

    vm.pending_timeouts.push(TimeoutInfo {
        pid: proc.pid,
        dst_reg: dst,
        result_reg,
        expiry_ms,
    });

    // BlockedOnTimeout: the scheduler will NOT revert IP, so the process resumes
    // after this instruction when it wakes up (either on timeout or incoming message).
    ExecutionResult::BlockedOnTimeout
}

pub(super) fn exec_link(
    vm: &mut IonVM,
    proc: &mut Process,
    target_pid: usize,
    ret_reg: usize,
) -> ExecutionResult {
    if let Some(target_rc) = vm.processes.get(&target_pid) {
        let target = target_rc.borrow();
        if target.status == ProcessStatus::Exited {
            let val = target
                .last_result
                .clone()
                .unwrap_or(Value::Primitive(Primitive::Undefined));
            drop(target);
            proc.frames
                .last_mut()
                .expect("active frame required")
                .registers[ret_reg] = val;
            return ExecutionResult::Continue;
        }
        // Target is still running — block and retry when rescheduled
        ExecutionResult::Blocked
    } else {
        ExecutionResult::Error(format!("link target process {} not found", target_pid))
    }
}

pub(super) fn exec_select(
    vm: &mut IonVM,
    proc: &mut Process,
    dst_reg: usize,
    pid_regs: Vec<usize>,
) -> ExecutionResult {
    let pids: Vec<usize> = pid_regs
        .iter()
        .filter_map(|&r| {
            if let Value::Process(p) = &proc.frames.last().unwrap().registers[r] {
                Some(p.borrow().pid)
            } else {
                None
            }
        })
        .collect();

    for &pid in &pids {
        if let Some(p) = vm.processes.get(&pid) {
            let p = p.borrow();
            if p.status == ProcessStatus::Exited {
                if let Some(v) = &p.last_result {
                    proc.frames.last_mut().unwrap().registers[dst_reg] = v.clone();
                    return ExecutionResult::Continue;
                }
            }
        }
    }
    ExecutionResult::Linked
}

pub(super) fn exec_select_with_kill(
    vm: &mut IonVM,
    proc: &mut Process,
    dst_reg: usize,
    pid_regs: Vec<usize>,
) -> ExecutionResult {
    let pids: Vec<usize> = pid_regs
        .iter()
        .filter_map(|&r| {
            if let Value::Process(p) = &proc.frames.last().unwrap().registers[r] {
                Some(p.borrow().pid)
            } else {
                None
            }
        })
        .collect();

    let mut winner = None;
    for &pid in &pids {
        if let Some(p) = vm.processes.get(&pid) {
            let p = p.borrow();
            if p.status == ProcessStatus::Exited {
                if let Some(v) = &p.last_result {
                    proc.frames.last_mut().unwrap().registers[dst_reg] = v.clone();
                    winner = Some(pid);
                    break;
                }
            }
        }
    }

    if let Some(winner_pid) = winner {
        for &pid in &pids {
            if pid != winner_pid {
                if let Some(p) = vm.processes.get(&pid) {
                    let mut p = p.borrow_mut();
                    p.alive = false;
                    p.status = ProcessStatus::Exited;
                }
            }
        }
    }

    ExecutionResult::Linked
}
