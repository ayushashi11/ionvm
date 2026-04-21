use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::cell::RefCell;
use std::rc::Rc;

use vmm::instruction::Pattern as VmPattern;
use vmm::value::{Object, ObjectInitArg, PropertyAccess};
use vmm::{
    Function as VmFunction, Instruction as VmInstruction, IonPackBuilder as VmIonPackBuilder,
    Primitive, Value as VmValue,
};

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
                VmInstruction::LoadConst(_, _) => "load_const",
                VmInstruction::Move(_, _) => "move",
                VmInstruction::Add(_, _, _) => "add",
                VmInstruction::Sub(_, _, _) => "sub",
                VmInstruction::Mul(_, _, _) => "mul",
                VmInstruction::Div(_, _, _) => "div",
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
                VmInstruction::MakeClosure(_, _, _, _) => "make_closure",
                VmInstruction::Return(_) => "return",
                VmInstruction::Spawn(_, _, _) => "spawn",
                VmInstruction::Send(_, _) => "send",
                VmInstruction::Receive(_) => "receive",
                VmInstruction::ReceiveWithTimeout(_, _, _) => "receive_with_timeout",
                VmInstruction::Link(_, _) => "link",
                VmInstruction::Match(_, _) => "match",
                VmInstruction::Yield => "yield",
                VmInstruction::Nop => "nop",
                VmInstruction::Select(_, _) => "select",
                VmInstruction::SelectWithKill(_, _) => "select_with_kill",
                VmInstruction::ArrayInit(_, _) => "array_init",
            }
            .to_string(),
        }
    }

    #[getter]
    pub fn args<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, pyo3::types::PyTuple>> {
        let mut list = vec![];
        if let InstrWrapper::Real(instr) = &self.inner {
            match instr {
                VmInstruction::Jump(offset) => {
                    list.push(offset.into_py(py));
                }
                VmInstruction::JumpIfTrue(reg, offset) => {
                    list.push(reg.into_py(py));
                    list.push(offset.into_py(py));
                }
                VmInstruction::JumpIfFalse(reg, offset) => {
                    list.push(reg.into_py(py));
                    list.push(offset.into_py(py));
                }
                VmInstruction::ArrayInit(dst, srcs) => {
                    list.push(dst.into_py(py));
                    list.push(srcs.clone().into_py(py));
                }
                VmInstruction::MakeClosure(dst, func, scope_id, captures) => {
                    list.push(dst.into_py(py));
                    list.push(func.into_py(py));
                    list.push(scope_id.clone().into_py(py));
                    let captures_list: Vec<(String, usize)> = captures.clone();
                    list.push(captures_list.into_py(py));
                }
                _ => {}
            }
        }
        Ok(pyo3::types::PyTuple::new_bound(py, list))
    }

    #[staticmethod]
    pub fn load_const(reg: usize, val: Value) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::LoadConst(reg, val.inner)),
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
    pub fn add(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Add(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn sub(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Sub(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn mul(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Mul(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn div(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Div(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn equal(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Equal(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn not_equal(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::NotEqual(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn less_than(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::LessThan(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn less_equal(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::LessEqual(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn greater_than(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GreaterThan(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn greater_equal(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GreaterEqual(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn and_instr(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::And(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn or_instr(dst: usize, a: usize, b: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Or(dst, a, b)),
        }
    }

    #[staticmethod]
    pub fn not_instr(dst: usize, a: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Not(dst, a)),
        }
    }

    #[staticmethod]
    pub fn get_prop(dst: usize, obj: usize, prop: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::GetProp(dst, obj, prop)),
        }
    }

    #[staticmethod]
    pub fn set_prop(obj: usize, prop: usize, val: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::SetProp(obj, prop, val)),
        }
    }

    #[staticmethod]
    pub fn call(dst: usize, func: usize, args: Vec<usize>) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Call(dst, func, args)),
        }
    }

    #[staticmethod]
    pub fn make_closure(
        dst: usize,
        func: usize,
        scope_id: String,
        captures: Vec<(String, usize)>,
    ) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::MakeClosure(
                dst, func, scope_id, captures,
            )),
        }
    }

    #[staticmethod]
    pub fn return_reg(reg: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Return(reg)),
        }
    }

    #[staticmethod]
    pub fn jump(offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Jump(offset)),
        }
    }

    #[staticmethod]
    pub fn jump_if_true(reg: usize, offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::JumpIfTrue(reg, offset)),
        }
    }

    #[staticmethod]
    pub fn jump_if_false(reg: usize, offset: isize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::JumpIfFalse(reg, offset)),
        }
    }

    #[staticmethod]
    pub fn spawn(dst: usize, func: usize, args: Vec<usize>) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Spawn(dst, func, args)),
        }
    }

    #[staticmethod]
    pub fn send(dst: usize, msg: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Send(dst, msg)),
        }
    }

    #[staticmethod]
    pub fn receive(dst: usize) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::Receive(dst)),
        }
    }

    #[staticmethod]
    pub fn match_instr(src: usize, patterns: Vec<Pattern>, jumps: Vec<isize>) -> PyResult<Self> {
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
            inner: InstrWrapper::Real(VmInstruction::Match(src, pairs)),
        })
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
    pub fn array_init(dst: usize, srcs: Vec<usize>) -> Self {
        Instruction {
            inner: InstrWrapper::Real(VmInstruction::ArrayInit(dst, srcs)),
        }
    }

    #[staticmethod]
    pub fn object_init(dst: usize, props: Vec<(String, Bound<'_, pyo3::PyAny>)>) -> PyResult<Self> {
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
            inner: InstrWrapper::Real(VmInstruction::ObjectInit(dst, vm_props)),
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

    Ok(())
}
