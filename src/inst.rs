#[derive(Debug,PartialEq)]
pub enum OpCode{
    Hlt,
    Load,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Jmp,
    Jmpb,
    Jmpf,
    Eq,
    Neq,
    Gt,
    Lt,
    Ge,
    Le,
    Jeq,
    Jneq,
    Igl
}
impl From<u16> for OpCode{
    fn from(v: u16) -> Self{
	match v{
	    1 => OpCode::Hlt,
	    2 => OpCode::Load,
	    3 => OpCode::Add,
	    4 => OpCode::Sub,
	    5 => OpCode::Mul,
	    6 => OpCode::Div,
	    7 => OpCode::Pow,
	    8 => OpCode::Jmp,
	    9 => OpCode::Jmpb,
	    10 => OpCode::Jmpf,
	    11 => OpCode::Eq,
	    12 => OpCode::Neq,
	    13 => OpCode::Gt,
	    14 => OpCode::Lt,
	    15 => OpCode::Ge,
	    16 => OpCode::Le,
	    17 => OpCode::Jeq,
	    18 => OpCode::Jneq,
	    _ => OpCode::Igl
	}
    }
}
#[derive(Debug,PartialEq)]
pub struct Inst{
    pub opc:OpCode
}
impl Inst{
    pub fn new(opc:OpCode) -> Self{
	Inst{opc}
    }
}
