use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::cell::RefCell;
use std::rc::Rc;

use vmm::instruction::{Instruction as VmInstruction, Operand as VmOperand, Pattern as VmPattern};
use vmm::value::{Object, ObjectInitArg, PropertyAccess};
use vmm::{
    Function as VmFunction, IonPackBuilder as VmIonPackBuilder,
    Primitive, Value as VmValue,
};

#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Operand {
    pub(crate) inner: VmOperand,
}

#[pymethods]
impl Operand {
    #[staticmethod]
    pub fn register(r: usize) -> Self {
        Operand {
            inner: VmOperand::Register(r),
        }
    }

    #[staticmethod]
    pub fn capture(scope: &str, name: &str) -> Self {
        Operand {
            inner: VmOperand::Capture(scope.to_string(), name.to_string()),
        }
    }

    #[staticmethod]
    pub fn argument(a: usize) -> Self {
        Operand {
            inner: VmOperand::Argument(a),
        }
    }

    #[staticmethod]
    pub fn constant(val: Value) -> Self {
        Operand {
            inner: VmOperand::Constant(val.inner),
        }
    }

    #[staticmethod]
    pub fn this() -> Self {
        Operand {
            inner: VmOperand::This,
        }
    }

    #[staticmethod]
    pub fn self_proc() -> Self {
        Operand {
            inner: VmOperand::SelfProc,
        }
    }

    pub fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }

    #[getter]
    pub fn register_index(&self) -> PyResult<usize> {
        match self.inner {
            VmOperand::Register(r) => Ok(r),
            _ => Err(PyValueError::new_err("Operand is not a register")),
        }
    }
}

#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Value {
    pub(crate) inner: VmValue,
}

#[pymethods]
impl Value {
    #[staticmethod]
    pub fn number(n: f64) -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::Number(n)),
        }
    }

    #[staticmethod]
    pub fn boolean(b: bool) -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::Boolean(b)),
        }
    }

    #[staticmethod]
    pub fn atom(s: &str) -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::Atom(s.to_string())),
        }
    }

    #[staticmethod]
    pub fn string(s: &str) -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::String(s.to_string())),
        }
    }

    #[staticmethod]
    pub fn unit() -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::Unit),
        }
    }

    #[staticmethod]
    pub fn undefined() -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::Undefined),
        }
    }

    #[staticmethod]
    pub fn array(items: Vec<Value>) -> Self {
        let vec: Vec<VmValue> = items.into_iter().map(|v| v.inner).collect();
        Value {
            inner: VmValue::Array(Rc::new(RefCell::new(vec))),
        }
    }

    #[staticmethod]
    pub fn object(dict: &Bound<'_, PyDict>) -> PyResult<Self> {
        let mut obj = Object::new(None);
        for (k, v) in dict.iter() {
            let key = k.extract::<String>()?;
            let val = v.extract::<Value>()?;
            obj.set_property(&key, val.inner);
        }
        Ok(Value {
            inner: VmValue::Object(Rc::new(RefCell::new(obj))),
        })
    }

    #[staticmethod]
    pub fn function_ref(name: &str) -> Self {
        Value {
            inner: VmValue::Primitive(Primitive::String(name.to_string())),
        }
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Pattern {
    pub(crate) inner: VmPattern,
}

#[pymethods]
impl Pattern {
    #[staticmethod]
    pub fn value(val: Value) -> Self {
        Pattern {
            inner: VmPattern::Value(val.inner),
        }
    }

    #[staticmethod]
    pub fn wildcard() -> Self {
        Pattern {
            inner: VmPattern::Wildcard,
        }
    }

    #[staticmethod]
    pub fn tuple(patterns: Vec<Pattern>) -> Self {
        Pattern {
            inner: VmPattern::Tuple(patterns.into_iter().map(|p| p.inner).collect()),
        }
    }

    #[staticmethod]
    pub fn array(patterns: Vec<Pattern>) -> Self {
        Pattern {
            inner: VmPattern::Array(patterns.into_iter().map(|p| p.inner).collect()),
        }
    }

    #[staticmethod]
    pub fn tagged_enum(tag: &str, pattern: Pattern) -> Self {
        Pattern {
            inner: VmPattern::TaggedEnum(tag.to_string(), Box::new(pattern.inner)),
        }
    }
}

#[derive(Clone)]
pub enum InstrWrapper {
    Real(VmInstruction),
    Placeholder(String),
}

#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Instruction {
    pub(crate) inner: InstrWrapper,
}

#[pymethods]
impl Instruction {
    #[getter]
    pub fn opcode(&self) -> String {
        match &self.inner {
            InstrWrapper::Placeholder(s) => s.clone(),
            InstrWrapper::Real(instr) => match instr {
                VmInstruction::Load(_, _) => "load",
                VmInstruction::Move(_, _) => "move",
                VmInstruction::Swap(_, _) => "swap",
                VmInstruction::MakeEnv(_, _) => "make_env",
                VmInstruction::Add(_, _, _) => "add",
                VmInstruction::Sub(_, _, _) => "sub",
                VmInstruction::Mul(_, _, _) => "mul",
                VmInstruction::Div(_, _, _) => "div",
                VmInstruction::Mod(_, _, _) => "mod",
                VmInstruction::Equal(_, _, _) => "equal",
                VmInstruction::NotEqual(_, _, _) => "not_equal",
                VmInstruction::LessThan(_, _, _) => "less_than",
                VmInstruction::LessEqual(_, _, _) => "less_equal",
                VmInstruction::GreaterThan(_, _, _) => "greater_than",
                VmInstruction::GreaterEqual(_, _, _) => "greater_equal",
                VmInstruction::And(_, _, _) => "and",
                VmInstruction::Or(_, _, _) => "or",
                VmInstruction::Not(_, _) => "not",
                VmInstruction::ObjectInit(_, _) => "object_init",
                VmInstruction::GetProp(_, _, _) => "get_prop",
                VmInstruction::SetProp(_, _, _) => "set_prop",
                VmInstruction::Jump(_) => "jump",
                VmInstruction::JumpIfTrue(_, _) => "jump_if_true",
                VmInstruction::JumpIfFalse(_, _) => "jump_if_false",
                VmInstruction::Call(_, _, _) => "call",
                VmInstruction::MakeClosure(_, _, _) => "make_closure",
                VmInstruction::Return(_) => "return",
                VmInstruction::Spawn(_, _, _) => "spawn",
                VmInstruction::Send(_, _) => "send",
                VmInstruction::Receive(_) => "receive",
                VmInstruction::ReceiveWithTimeout(_, _, _) => "receive_with_timeout",
                VmInstruction::Link(_, _) => "link",
                VmInstruction::Match(_, _) => "match",
                VmInstruction::Yield(_) => "yield",
                VmInstruction::Nop => "nop",
                VmInstruction::Select(_, _) => "select",
                VmInstruction::SelectWithKill(_, _) => "select_with_kill",
                VmInstruction::ArrayInit(_, _) => "array_init",
                VmInstruction::FloorDiv(_, _, _) => "floor_div",
                VmInstruction::IndexGet(_, _, _) => "index_get",
            }
            .to_string(),
        }
    }

    #[getter]
    pub fn args<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, pyo3::types::PyTuple>> {
        let mut list = vec![];
        if let InstrWrapper::Real(instr) = &self.inner {
            match instr {
                VmInstruction::Load(dst, src) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: src.clone() }.into_py(py));
                }
                VmInstruction::Move(dst, src) => {
                    list.push(dst.into_py(py));
                    list.push(src.into_py(py));
                }
                VmInstruction::Swap(a, b) => {
                    list.push(Operand { inner: a.clone() }.into_py(py));
                    list.push(Operand { inner: b.clone() }.into_py(py));
                }
                VmInstruction::MakeEnv(scope_id, captures) => {
                    list.push(scope_id.clone().into_py(py));
                    let caps_list: Vec<(String, Operand)> = captures
                        .iter()
                        .map(|(n, o)| (n.clone(), Operand { inner: o.clone() }))
                        .collect();
                    list.push(caps_list.into_py(py));
                }
                VmInstruction::Add(dst, a, b)
                | VmInstruction::Sub(dst, a, b)
                | VmInstruction::Mul(dst, a, b)
                | VmInstruction::Div(dst, a, b)
                | VmInstruction::Mod(dst, a, b)
                | VmInstruction::Equal(dst, a, b)
                | VmInstruction::NotEqual(dst, a, b)
                | VmInstruction::LessThan(dst, a, b)
                | VmInstruction::LessEqual(dst, a, b)
                | VmInstruction::GreaterThan(dst, a, b)
                | VmInstruction::GreaterEqual(dst, a, b)
                | VmInstruction::And(dst, a, b)
                | VmInstruction::Or(dst, a, b) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: a.clone() }.into_py(py));
                    list.push(Operand { inner: b.clone() }.into_py(py));
                }
                VmInstruction::Not(dst, src) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: src.clone() }.into_py(py));
                }
                VmInstruction::Jump(offset) => {
                    list.push(offset.into_py(py));
                }
                VmInstruction::JumpIfTrue(cond, offset) | VmInstruction::JumpIfFalse(cond, offset) => {
                    list.push(Operand { inner: cond.clone() }.into_py(py));
                    list.push(offset.into_py(py));
                }
                VmInstruction::ArrayInit(dst, srcs) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    let srcs_list: Vec<Operand> =
                        srcs.iter().map(|s| Operand { inner: s.clone() }).collect();
                    list.push(srcs_list.into_py(py));
                }
                VmInstruction::ObjectInit(dst, kvs) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    // We can't easily return ObjectInitArg back to Python yet,
                    // maybe just return raw info if needed.
                }
                VmInstruction::GetProp(dst, obj, key)
                | VmInstruction::IndexGet(dst, obj, key) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: obj.clone() }.into_py(py));
                    list.push(Operand { inner: key.clone() }.into_py(py));
                }
                VmInstruction::FloorDiv(dst, a, b) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: a.clone() }.into_py(py));
                    list.push(Operand { inner: b.clone() }.into_py(py));
                }
                VmInstruction::SetProp(obj, key, val) => {
                    list.push(Operand { inner: obj.clone() }.into_py(py));
                    list.push(Operand { inner: key.clone() }.into_py(py));
                    list.push(Operand { inner: val.clone() }.into_py(py));
                }
                VmInstruction::Call(callee, args, dst) => {
                    list.push(Operand { inner: callee.clone() }.into_py(py));
                    let args_list: Vec<Operand> =
                        args.iter().map(|a| Operand { inner: a.clone() }).collect();
                    list.push(args_list.into_py(py));
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                }
                VmInstruction::MakeClosure(dst, func, scope_id) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: func.clone() }.into_py(py));
                    list.push(scope_id.clone().into_py(py));
                }
                VmInstruction::Return(reg) | VmInstruction::Receive(reg) | VmInstruction::Yield(reg) => {
                    list.push(Operand { inner: reg.clone() }.into_py(py));
                }
                VmInstruction::Spawn(func, args, dst) => {
                    list.push(Operand { inner: func.clone() }.into_py(py));
                    let args_list: Vec<Operand> =
                        args.iter().map(|a| Operand { inner: a.clone() }).collect();
                    list.push(args_list.into_py(py));
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                }
                VmInstruction::Send(pid, msg) => {
                    list.push(Operand { inner: pid.clone() }.into_py(py));
                    list.push(Operand { inner: msg.clone() }.into_py(py));
                }
                VmInstruction::ReceiveWithTimeout(dst, timeout, result) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    list.push(Operand { inner: timeout.clone() }.into_py(py));
                    list.push(Operand { inner: result.clone() }.into_py(py));
                }
                VmInstruction::Link(pid, dst) => {
                    list.push(Operand { inner: pid.clone() }.into_py(py));
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                }
                VmInstruction::Match(src, _) => {
                    list.push(Operand { inner: src.clone() }.into_py(py));
                }
                VmInstruction::Select(dst, pids) | VmInstruction::SelectWithKill(dst, pids) => {
                    list.push(Operand { inner: dst.clone() }.into_py(py));
                    let pids_list: Vec<Operand> =
                        pids.iter().map(|p| Operand { inner: p.clone() }).collect();
                    list.push(pids_list.into_py(py));
                }
                VmInstruction::Nop => {}
            }
        }
        Ok(pyo3::types::PyTuple::new_bound(py, list))
    }

    #[staticmethod]
    pub fn load(dst: Operand, src: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Load(dst.inner, src.inner)),
        }
    }

    #[staticmethod]
    pub fn swap(a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Swap(a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn make_env(scope_id: String, captures: Vec<(String, Operand)>) -> Self {
        let vm_captures = captures.into_iter().map(|(n, o)| (n, o.inner)).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::MakeEnv(scope_id, vm_captures)),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (dst, src))]
    pub fn move_reg(dst: usize, src: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Move(dst, src)),
        }
    }

    #[staticmethod]
    pub fn add(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Add(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn sub(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Sub(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn mul(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Mul(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn div(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Div(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn floor_div(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::FloorDiv(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn index_get(dst: Operand, obj: Operand, key: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::IndexGet(dst.inner, obj.inner, key.inner)),
        }
    }

    #[staticmethod]
    pub fn mod_instr(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Mod(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn equal(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Equal(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn not_equal(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::NotEqual(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn less_than(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::LessThan(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn less_equal(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::LessEqual(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn greater_than(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GreaterThan(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn greater_equal(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GreaterEqual(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn and_instr(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::And(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn or_instr(dst: Operand, a: Operand, b: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Or(dst.inner, a.inner, b.inner)),
        }
    }

    #[staticmethod]
    pub fn not_instr(dst: Operand, a: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Not(dst.inner, a.inner)),
        }
    }

    #[staticmethod]
    pub fn get_prop(dst: Operand, obj: Operand, prop: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GetProp(dst.inner, obj.inner, prop.inner)),
        }
    }

    #[staticmethod]
    pub fn set_prop(obj: Operand, prop: Operand, val: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::SetProp(obj.inner, prop.inner, val.inner)),
        }
    }

    #[staticmethod]
    pub fn call(callee: Operand, args: Vec<Operand>, dst: Operand) -> Self {
        let vm_args = args.into_iter().map(|o| o.inner).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Call(callee.inner, vm_args, dst.inner)),
        }
    }

    #[staticmethod]
    pub fn make_closure(dst: Operand, func: Operand, scope_id: String) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::MakeClosure(dst.inner, func.inner, scope_id)),
        }
    }

    #[staticmethod]
    pub fn return_reg(reg: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Return(reg.inner)),
        }
    }

    #[staticmethod]
    pub fn jump(offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Jump(offset)),
        }
    }

    #[staticmethod]
    pub fn jump_if_true(cond: Operand, offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::JumpIfTrue(cond.inner, offset)),
        }
    }

    #[staticmethod]
    pub fn jump_if_false(cond: Operand, offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::JumpIfFalse(cond.inner, offset)),
        }
    }

    #[staticmethod]
    pub fn spawn(func: Operand, args: Vec<Operand>, dst: Operand) -> Self {
        let vm_args = args.into_iter().map(|o| o.inner).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Spawn(func.inner, vm_args, dst.inner)),
        }
    }

    #[staticmethod]
    pub fn send(pid: Operand, msg: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Send(pid.inner, msg.inner)),
        }
    }

    #[staticmethod]
    pub fn receive(dst: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Receive(dst.inner)),
        }
    }

    #[staticmethod]
    pub fn receive_with_timeout(dst: Operand, timeout: Operand, result: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::ReceiveWithTimeout(
                dst.inner,
                timeout.inner,
                result.inner,
            )),
        }
    }

    #[staticmethod]
    pub fn link(pid: Operand, dst: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Link(pid.inner, dst.inner)),
        }
    }

    #[staticmethod]
    pub fn match_instr(src: Operand, patterns: Vec<Pattern>, jumps: Vec<isize>) -> PyResult<Self> {
        if patterns.len() != jumps.len() {
            return Err(PyValueError::new_err(
                "patterns and jumps must have same length",
            ));
        }
        let pairs = patterns
            .into_iter()
            .zip(jumps.into_iter())
            .map(|(p, j)| (p.inner, j))
            .collect();
        Ok(Instruction {
            inner: InstrWrapper::Real(VmInstruction::Match(src.inner, pairs)),
        })
    }

    #[staticmethod]
    pub fn yield_instr(reg: Operand) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Yield(reg.inner)),
        }
    }

    #[staticmethod]
    pub fn select(dst: Operand, pids: Vec<Operand>) -> Self {
        let vm_pids = pids.into_iter().map(|o| o.inner).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Select(dst.inner, vm_pids)),
        }
    }

    #[staticmethod]
    pub fn select_with_kill(dst: Operand, pids: Vec<Operand>) -> Self {
        let vm_pids = pids.into_iter().map(|o| o.inner).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::SelectWithKill(dst.inner, vm_pids)),
        }
    }

    #[staticmethod]
    pub fn break_instr() -> Self {
        Instruction {
            inner: InstrWrapper::Placeholder("break".to_string()),
        }
    }

    #[staticmethod]
    pub fn continue_instr() -> Self {
        Instruction {
            inner: InstrWrapper::Placeholder("continue".to_string()),
        }
    }

    #[staticmethod]
    pub fn nop() -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Nop),
        }
    }

    #[staticmethod]
    pub fn array_init(dst: Operand, srcs: Vec<Operand>) -> Self {
        let vm_srcs = srcs.into_iter().map(|o| o.inner).collect();
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::ArrayInit(dst.inner, vm_srcs)),
        }
    }

    #[staticmethod]
    pub fn object_init(dst: Operand, props: Vec<(String, Bound<'_, pyo3::PyAny>)>) -> PyResult<Self> {
        let mut vm_props = Vec::new();
        for (name, arg_tuple) in props {
            let tup = arg_tuple.downcast::<PyTuple>()?;
            let kind = tup.get_item(0)?.extract::<String>()?;
            let access = PropertyAccess::Public; // just use public for now

            let obj_init_arg = if kind == "reg" {
                let reg = tup.get_item(1)?.extract::<usize>()?;
                ObjectInitArg::RegisterWithAccess(reg, access)
            } else if kind == "val" {
                let val = tup.get_item(1)?.extract::<Value>()?;
                ObjectInitArg::ValueWithAccess(val.inner, access)
            } else {
                return Err(PyValueError::new_err(format!(
                    "Invalid object_init arg kind: {}",
                    kind
                )));
            };

            vm_props.push((name, obj_init_arg));
        }
        Ok(Instruction {
            inner: InstrWrapper::Real(VmInstruction::ObjectInit(dst.inner, vm_props)),
        })
    }

    pub fn serialize(&self, _writer: Bound<'_, pyo3::PyAny>) -> PyResult<()> {
        Ok(())
    }

    pub fn __repr__(&self) -> String {
        match &self.inner {
            InstrWrapper::Real(i) => format!("{:?}", i),
            InstrWrapper::Placeholder(s) => format!("Placeholder({})", s),
        }
    }
}

#[pyclass(unsendable)]
#[derive(Clone)]
pub struct Function {
    pub(crate) inner: VmFunction,
}

#[pymethods]
impl Function {
    #[new]
    #[pyo3(signature = (name=None, arity=0, extra_regs=0, instructions=vec![]))]
    pub fn new(
        name: Option<String>,
        arity: usize,
        extra_regs: usize,
        instructions: Vec<Instruction>,
    ) -> Self {
        let vm_instructions = instructions
            .into_iter()
            .filter_map(|i| match i.inner {
                InstrWrapper::Real(r) => Some(r),
                InstrWrapper::Placeholder(_) => None,
            })
            .collect();
        Function {
            inner: VmFunction::new_bytecode(name, arity, extra_regs, vm_instructions),
        }
    }

    #[getter]
    pub fn name(&self) -> Option<String> {
        self.inner.name.clone()
    }

    #[getter]
    pub fn arity(&self) -> usize {
        self.inner.arity
    }

    #[getter]
    pub fn extra_regs(&self) -> usize {
        self.inner.extra_regs
    }

    #[getter]
    pub fn instructions(&self) -> PyResult<Vec<Instruction>> {
        if let vmm::value::FunctionType::Bytecode { bytecode } = &self.inner.function_type {
            Ok(bytecode
                .iter()
                .map(|i| Instruction {
                    inner: InstrWrapper::Real(i.clone()),
                })
                .collect())
        } else {
            Ok(vec![])
        }
    }

    pub fn serialize(&self, _writer: Bound<'_, pyo3::PyAny>) -> PyResult<()> {
        Ok(())
    }

    pub fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pyclass(unsendable)]
pub struct IonPackBuilder {
    pub(crate) inner: Option<VmIonPackBuilder>,
}

#[pymethods]
impl IonPackBuilder {
    #[new]
    pub fn new(name: String, version: String) -> Self {
        IonPackBuilder {
            inner: Some(VmIonPackBuilder::new(name, version)),
        }
    }

    pub fn main_class(&mut self, main_class: String) -> PyResult<()> {
        if let Some(builder) = self.inner.take() {
            self.inner = Some(builder.main_class(main_class));
        }
        Ok(())
    }

    pub fn entry_point(&mut self, entry_point: String) -> PyResult<()> {
        if let Some(builder) = self.inner.take() {
            self.inner = Some(builder.entry_point(entry_point));
        }
        Ok(())
    }

    pub fn description(&mut self, description: String) -> PyResult<()> {
        if let Some(builder) = self.inner.take() {
            self.inner = Some(builder.description(description));
        }
        Ok(())
    }

    pub fn author(&mut self, author: String) -> PyResult<()> {
        if let Some(builder) = self.inner.take() {
            self.inner = Some(builder.author(author));
        }
        Ok(())
    }

    pub fn add_class(&mut self, name: String, function: &Function) -> PyResult<()> {
        if let Some(mut builder) = self.inner.take() {
            let res = builder.add_class(&name, &function.inner);
            match res {
                Ok(_) => {
                    self.inner = Some(builder);
                    Ok(())
                }
                Err(e) => Err(PyValueError::new_err(format!(
                    "Failed to add class: {:?}",
                    e
                ))),
            }
        } else {
            Err(PyValueError::new_err("Builder consumed"))
        }
    }

    pub fn add_multi_function_class(
        &mut self,
        name: String,
        functions: Vec<Function>,
    ) -> PyResult<()> {
        if let Some(mut builder) = self.inner.take() {
            let vm_functions: Vec<_> = functions.into_iter().map(|f| f.inner).collect();
            let res = builder.add_multi_function_class(&name, &vm_functions);
            match res {
                Ok(_) => {
                    self.inner = Some(builder);
                    Ok(())
                }
                Err(e) => Err(PyValueError::new_err(format!(
                    "Failed to add multi-function class: {:?}",
                    e
                ))),
            }
        } else {
            Err(PyValueError::new_err("Builder consumed"))
        }
    }

    pub fn add_source(&mut self, filename: String, content: String) -> PyResult<()> {
        if let Some(mut builder) = self.inner.take() {
            builder.add_source(&filename, content);
            self.inner = Some(builder);
            Ok(())
        } else {
            Err(PyValueError::new_err("Builder consumed"))
        }
    }

    pub fn build(&mut self, file: Bound<'_, pyo3::PyAny>) -> PyResult<()> {
        if let Some(mut builder) = self.inner.take() {
            let mut cursor = std::io::Cursor::new(Vec::new());
            match builder.build(&mut cursor) {
                Ok(_) => {
                    let bytes = cursor.into_inner();
                    file.call_method1(
                        "write",
                        (pyo3::types::PyBytes::new_bound(file.py(), &bytes),),
                    )?;
                    Ok(())
                }
                Err(e) => Err(PyValueError::new_err(format!(
                    "Failed to build IonPack: {:?}",
                    e
                ))),
            }
        } else {
            Err(PyValueError::new_err("Builder consumed"))
        }
    }
}

#[pymodule]
fn ionvm_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Value>()?;
    m.add_class::<Operand>()?;
    m.add_class::<Pattern>()?;
    m.add_class::<Instruction>()?;
    m.add_class::<Function>()?;
    m.add_class::<IonPackBuilder>()?;

    // Alias move_reg to move
    let instr_cls = m.getattr("Instruction")?;
    instr_cls.setattr("move", instr_cls.getattr("move_reg")?)?;
    instr_cls.setattr("and_", instr_cls.getattr("and_instr")?)?;
    instr_cls.setattr("or_", instr_cls.getattr("or_instr")?)?;
    instr_cls.setattr("not_", instr_cls.getattr("not_instr")?)?;
    instr_cls.setattr("match", instr_cls.getattr("match_instr")?)?;
    instr_cls.setattr("mod", instr_cls.getattr("mod_instr")?)?;
    instr_cls.setattr("floor_div", instr_cls.getattr("floor_div")?)?;
    instr_cls.setattr("index_get", instr_cls.getattr("index_get")?)?;
    instr_cls.setattr("yield", instr_cls.getattr("yield_instr")?)?;

    Ok(())
}
