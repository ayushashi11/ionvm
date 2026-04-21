use super::{Function, Primitive, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::rc::Rc;

// A single call frame on the process stack
#[derive(Debug, Clone)]
pub struct Frame {
    pub registers: Vec<Value>,
    pub ip: usize,
    pub function: Rc<RefCell<Function>>,
    pub return_value: Option<Value>,
    pub caller_return_reg: Option<usize>,
    pub scope_environments: HashMap<String, Rc<RefCell<HashMap<String, Value>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Runnable,
    WaitingForMessage,
    Suspended,
    Exited,
}

#[derive(Debug)]
pub struct Process {
    pub pid: usize,
    pub frames: Vec<Frame>,
    // FIFO mailbox: push_back to enqueue, pop_front to dequeue
    pub mailbox: VecDeque<Value>,
    pub links: Vec<usize>,
    pub alive: bool,
    pub last_result: Option<Value>,
    // Remaining reduction budget for the current scheduling turn
    pub budget: u32,
    pub status: ProcessStatus,
    // Closure environments stored per-process, indexed by slot.
    // Each entry is a flat list of captured values.
    // Future closure/iterator function types will index into this.
    pub environments: Vec<Vec<Value>>,
}

impl Process {
    pub fn new(pid: usize, function: Rc<RefCell<Function>>, args: Vec<Value>) -> Self {
        let total_regs = function.borrow().total_registers().max(16);
        let mut registers = args;
        registers.resize(total_regs, Value::Primitive(Primitive::Undefined));
        let frame = Frame {
            registers,
            ip: 0,
            function,
            return_value: None,
            caller_return_reg: None,
            scope_environments: HashMap::new(),
        };
        Process {
            pid,
            frames: vec![frame],
            mailbox: VecDeque::new(),
            links: Vec::new(),
            alive: true,
            last_result: None,
            budget: 0,
            status: ProcessStatus::Runnable,
            environments: Vec::new(),
        }
    }

    pub fn reset_budget(&mut self, amount: u32) {
        self.budget = amount;
    }

    // Subtract cost from the budget. Returns true when budget hits zero.
    pub fn spend(&mut self, cost: u32) -> bool {
        self.budget = self.budget.saturating_sub(cost);
        self.budget == 0
    }

    pub fn is_schedulable(&self) -> bool {
        self.alive && self.status == ProcessStatus::Runnable
    }
}
