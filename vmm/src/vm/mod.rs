pub mod ops;
pub mod scheduler;
pub mod timeout;

use crate::value::{Function, Primitive, Process, Value};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use timeout::TimeoutInfo;
use vm_ffi::FfiRegistry;

// Re-export so that code using `crate::vm::{Instruction, Pattern}` keeps working
pub use crate::instruction::{Instruction, Pattern};

// Result returned after executing one instruction or a process slice
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    // Keep running in the same process
    Continue,
    // Yield to the next process voluntarily
    Pass,
    // Reduction budget exhausted; reschedule
    BudgetExhausted,
    // Blocked waiting for a message; revert IP and retry on wake
    Blocked,
    // Blocked on ReceiveWithTimeout; do NOT revert IP on wake
    BlockedOnTimeout,
    // Waiting for another process to finish (Link / Select)
    Linked,
    // Process exited cleanly with a return value
    Exited(Value),
    // Unrecoverable error in this process
    Error(String),
}

pub struct IonVM {
    pub processes: HashMap<usize, Rc<RefCell<Process>>>,
    pub run_queue: VecDeque<usize>,
    pub next_pid: usize,
    // Reduction budget granted to each process per scheduling turn.
    // Matches the BEAM default of 2000.
    pub reduction_budget: u32,
    pub scheduler_passes: u64,
    pub ffi_registry: FfiRegistry,
    pub debug: bool,
    pub(crate) pending_timeouts: Vec<TimeoutInfo>,
}

impl IonVM {
    pub fn new() -> Self {
        IonVM {
            processes: HashMap::new(),
            run_queue: VecDeque::new(),
            next_pid: 1,
            reduction_budget: 4, //2000,
            scheduler_passes: 0,
            ffi_registry: FfiRegistry::with_stdlib(),
            debug: false,
            pending_timeouts: Vec::new(),
        }
    }

    pub fn with_debug() -> Self {
        let mut vm = Self::new();
        vm.debug = true;
        vm
    }

    pub fn set_debug(&mut self, enabled: bool) {
        self.debug = enabled;
    }

    pub fn spawn_process(&mut self, function: Rc<RefCell<Function>>, args: Vec<Value>) -> usize {
        let pid = self.next_pid;
        self.next_pid += 1;

        if self.debug {
            eprintln!(
                "[VM] spawning process {} fn={:?}",
                pid,
                function.borrow().name.clone()
            );
        }

        let process = Rc::new(RefCell::new(Process::new(pid, function, args)));
        self.processes.insert(pid, process);
        self.run_queue.push_back(pid);
        pid
    }

    pub fn spawn_main_process(&mut self, function: Function) -> Result<Value, String> {
        let pid = self.spawn_process(Rc::new(RefCell::new(function)), vec![]);

        loop {
            self.run();

            if let Some(proc) = self.processes.get(&pid) {
                let proc = proc.borrow();
                if !proc.alive {
                    return Ok(proc
                        .last_result
                        .clone()
                        .unwrap_or(Value::Primitive(Primitive::Unit)));
                }
            } else {
                return Ok(Value::Primitive(Primitive::Unit));
            }

            if self.run_queue.is_empty() {
                break;
            }
        }

        Ok(Value::Primitive(Primitive::Unit))
    }

    // Get a named FFI function as a first-class Value
    pub fn get_ffi_function(&self, name: &str) -> Option<Value> {
        self.ffi_registry
            .get_function_info(name)
            .map(|(n, arity, _)| {
                Value::Function(Rc::new(RefCell::new(Function::new_ffi(
                    Some(n.to_string()),
                    arity,
                    n.to_string(),
                ))))
            })
    }

    pub fn get_all_ffi_functions(&self) -> HashMap<String, Value> {
        self.ffi_registry
            .list_functions()
            .iter()
            .filter_map(|&name| self.get_ffi_function(name).map(|f| (name.to_string(), f)))
            .collect()
    }

    // Resolve __vm: intrinsic atoms to runtime values.
    // Only truly dynamic values that depend on the running process belong here.
    // __stdlib: atoms are resolved at load time by the IonPack loader instead.
    pub(crate) fn resolve_vm_atom(&self, atom: &str, pid: usize) -> Option<Value> {
        let cmd = atom.strip_prefix("__vm:")?;
        Some(match cmd {
            "self" => Value::Process(self.processes.get(&pid)?.clone()),
            "pid" => Value::Primitive(Primitive::Number(pid as f64)),
            "processes" => Value::Primitive(Primitive::Number(self.processes.len() as f64)),
            "scheduler_passes" => Value::Primitive(Primitive::Number(self.scheduler_passes as f64)),
            _ => return None,
        })
    }
}
