use super::{ExecutionResult, IonVM};
use crate::value::process::{Process, ProcessStatus};
use crate::value::{FunctionType, Primitive, Value};
use std::cell::RefCell;
use std::rc::Rc;

impl IonVM {
    // Main scheduling loop — runs until there are no more runnable processes
    pub fn run(&mut self) {
        while self.has_runnable_processes() {
            self.scheduler_passes += 1;

            if let Some(pid) = self.run_queue.pop_front() {
                if self.debug {
                    eprintln!(
                        "[VM] scheduling process {} (pass {})",
                        pid, self.scheduler_passes
                    );
                }
                if let Some(proc_rc) = self.processes.get(&pid).cloned() {
                    let result = self.execute_process_slice(proc_rc);
                    self.handle_execution_result(pid, result);
                }
            }

            self.check_blocked_processes();
        }
    }

    fn has_runnable_processes(&mut self) -> bool {
        self.handle_expired_timeouts();
        // A process is "runnable" if it's already Runnable OR if it's blocked but has mail,
        // which means check_blocked_processes() can wake it during this pass.
        self.processes.values().any(|p| {
            let proc = p.borrow();
            proc.is_schedulable()
                || (proc.status == ProcessStatus::WaitingForMessage && !proc.mailbox.is_empty())
        })
    }

    fn execute_process_slice(&mut self, proc_rc: Rc<RefCell<Process>>) -> ExecutionResult {
        let mut proc = proc_rc.borrow_mut();
        proc.reset_budget(self.reduction_budget);

        while proc.budget > 0 && proc.status == ProcessStatus::Runnable {
            if proc.frames.is_empty() {
                return ExecutionResult::Exited(Value::Primitive(Primitive::Unit));
            }

            // Fetch the next instruction
            let (instruction, implicit_return) = {
                let frame = proc.frames.last_mut().unwrap();
                let ip = frame.ip;
                let func = frame.function.borrow();

                match &func.function_type {
                    FunctionType::Bytecode { bytecode } => {
                        if ip >= bytecode.len() {
                            // Implicit return at end of function
                            let ret = frame
                                .return_value
                                .clone()
                                .unwrap_or(Value::Primitive(Primitive::Unit));
                            let caller_return_reg = frame.caller_return_reg;
                            (None, Some((ret, caller_return_reg)))
                        } else {
                            (Some(bytecode[ip].clone()), None)
                        }
                    }
                    FunctionType::Ffi { .. } => {
                        return ExecutionResult::Error(
                            "FFI function found on the frame stack; this is a bug".to_string(),
                        );
                    }
                }
            };

            if let Some((ret, caller_return_reg)) = implicit_return {
                proc.frames.pop();
                if let Some(caller) = proc.frames.last_mut() {
                    if let Some(reg) = caller_return_reg {
                        caller.registers[reg] = ret;
                    }
                    continue;
                } else {
                    return ExecutionResult::Exited(ret);
                }
            }

            let instruction = instruction.unwrap();

            // Advance IP before execution so jumps can compute relative offsets correctly
            if let Some(frame) = proc.frames.last_mut() {
                frame.ip += 1;
            }

            let cost = instruction.reduction_cost();
            let result = self.execute_instruction(&mut proc, instruction);
            let exhausted = proc.spend(cost);

            match result {
                ExecutionResult::Continue => {
                    if exhausted {
                        return ExecutionResult::BudgetExhausted;
                    }
                }
                ExecutionResult::Blocked => {
                    // Revert IP so the blocking instruction retries on next wake
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.ip -= 1;
                    }
                    proc.status = ProcessStatus::WaitingForMessage;
                    return ExecutionResult::Blocked;
                }
                ExecutionResult::BlockedOnTimeout => {
                    // Do NOT revert IP — the process continues after the instruction on wake
                    proc.status = ProcessStatus::WaitingForMessage;
                    return ExecutionResult::Blocked;
                }
                other => return other,
            }
        }

        ExecutionResult::BudgetExhausted
    }

    fn handle_execution_result(&mut self, pid: usize, result: ExecutionResult) {
        match result {
            ExecutionResult::BudgetExhausted
            | ExecutionResult::Continue
            | ExecutionResult::Pass => {
                self.run_queue.push_back(pid);
            }
            ExecutionResult::Blocked => {
                if let Some(p) = self.processes.get(&pid) {
                    p.borrow_mut().status = ProcessStatus::WaitingForMessage;
                }
            }
            ExecutionResult::Exited(val) => {
                if let Some(p) = self.processes.get(&pid) {
                    let mut p = p.borrow_mut();
                    p.alive = false;
                    p.status = ProcessStatus::Exited;
                    p.last_result = Some(val);
                }
            }
            ExecutionResult::Error(msg) => {
                eprintln!("[VM] process {} error: {}", pid, msg);
                if let Some(p) = self.processes.get(&pid) {
                    let mut p = p.borrow_mut();
                    p.alive = false;
                    p.status = ProcessStatus::Exited;
                }
            }
            ExecutionResult::Linked => {
                if let Some(p) = self.processes.get(&pid) {
                    p.borrow_mut().status = ProcessStatus::Suspended;
                }
            }
            // BlockedOnTimeout is converted to Blocked inside execute_process_slice;
            // it should never reach handle_execution_result directly.
            ExecutionResult::BlockedOnTimeout => {}
        }
    }

    // Scan for blocked processes that now have mail.
    // Called after every scheduling pass to avoid starvation.
    pub fn check_blocked_processes(&mut self) {
        let pids: Vec<usize> = self.processes.keys().copied().collect();
        for pid in pids {
            let needs_unblock = {
                let proc = self.processes.get(&pid).unwrap().borrow();
                proc.status == ProcessStatus::WaitingForMessage && !proc.mailbox.is_empty()
            };
            if !needs_unblock {
                continue;
            }

            // Check whether this process was blocked on ReceiveWithTimeout
            let timeout_regs = self
                .pending_timeouts
                .iter()
                .find(|t| t.pid == pid)
                .map(|t| (t.dst_reg, t.result_reg));

            if let Some((dst, result)) = timeout_regs {
                // ReceiveWithTimeout: consume the message and deliver directly to registers
                let proc_rc = self.processes.get(&pid).unwrap().clone();
                let mut proc = proc_rc.borrow_mut();
                if let Some(msg) = proc.mailbox.pop_front() {
                    if let Some(frame) = proc.frames.last_mut() {
                        frame.registers[dst] = msg;
                        frame.registers[result] = Value::Primitive(Primitive::Boolean(true));
                    }
                }
                proc.status = ProcessStatus::Runnable;
                drop(proc);
                self.pending_timeouts.retain(|t| t.pid != pid);
            } else {
                // Regular Receive: just wake. The instruction retries and consumes the message.
                if let Some(proc_rc) = self.processes.get(&pid) {
                    proc_rc.borrow_mut().status = ProcessStatus::Runnable;
                }
            }

            if !self.run_queue.contains(&pid) {
                self.run_queue.push_back(pid);
            }
        }
    }

    // Make a process runnable and add it to the scheduling queue
    pub(crate) fn wake_process(&mut self, pid: usize) {
        if let Some(proc_rc) = self.processes.get(&pid) {
            proc_rc.borrow_mut().status = ProcessStatus::Runnable;
        }
        if !self.run_queue.contains(&pid) {
            self.run_queue.push_back(pid);
        }
    }
}
