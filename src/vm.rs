use crate::inst::*;
pub struct VM{
    pub regs:[i32;32],
    pub pc:usize,
    pub rem:i32,
    pub program:Vec<u16>
}
impl VM{
    pub fn new() -> Self{
	VM{
	    regs:[0;32],
	    program:vec![],
	    pc:0,
	    rem:0
	}
    }
    pub fn from(prog: Vec<u16>) -> Self{
	VM{
	    regs:[0;32],
	    program:prog,
	    pc:0,
	    rem:0
	}
    }
    fn decode_opcode(&mut self) -> OpCode{
	let opc=OpCode::from(self.program[self.pc]);
	self.pc+=1;
	//println!("{}",self.pc);
	opc
    }
    fn next_16_bits(&mut self) -> u16{
	let result=self.program[self.pc];
	self.pc+=1;
	//println!("{}",self.pc);
	result
    }
    fn next_32_bits(&mut self) -> u32{
	let result=((self.program[self.pc] as u32)<<16) | self.program[self.pc+1] as u32;
	self.pc+=2;
	//println!("{}",self.pc);
	result
    }
    pub fn run(&mut self){
	let mut is_done=false;
	while !is_done{
	    is_done=self.execute_instruction();
	}
    }
    pub fn run_once(&mut self){
	self.execute_instruction();
    }
    fn execute_instruction(&mut self) -> bool{
	if self.pc>=self.program.len(){
	    return true;
	}
	match self.decode_opcode(){
	    OpCode::Load => {
		let reg=self.next_16_bits() as usize;
		let v=self.next_32_bits() as i32;
		match reg{
		    0..31=>{
			self.regs[reg]=v;
		    },
		    _=>{
			//TODO: HEAP
		    }
		false
	    },
	    OpCode::Hlt => {
		println!("Hlt encountered\nleaving...");
		true
	    },
	    OpCode::Add => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1+reg2) as i32;
		false
	    },
	    OpCode::Sub => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1-reg2) as i32;
		false
	    },
	    OpCode::Mul => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1*reg2) as i32;
		false
	    },
	    OpCode::Div => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=reg1/reg2;
		self.rem=reg1%reg2;
		false
	    },
	    OpCode::Pow => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=reg1.pow(reg2 as u32) as i32;
		false
	    },
	    OpCode::Jmp => {
		self.pc=self.regs[self.next_16_bits() as usize] as usize;
		false
	    },
	    OpCode::Jmpb => {
		self.pc-=self.regs[self.next_16_bits() as usize] as usize;
		false
	    },
	    OpCode::Jmpf => {
		self.pc+=self.regs[self.next_16_bits() as usize] as usize;
		false
	    },
	    OpCode::Eq => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1==reg2) as i32;
		false
	    },
	    OpCode::Neq => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1!=reg2) as i32;
		false
	    },
	    OpCode::Ge => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1>=reg2) as i32;
		false
	    },
	    OpCode::Le => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1<=reg2) as i32;
		false
	    },
	    OpCode::Gt => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1>reg2) as i32;
		false
	    },
	    OpCode::Lt => {
		let reg1=self.regs[self.next_16_bits() as usize];
		let reg2=self.regs[self.next_16_bits() as usize];
		self.regs[self.next_16_bits() as usize]=(reg1<reg2) as i32;
		false
	    },
	    OpCode::Jeq => {
		println!("jeq");
		if self.regs[self.next_16_bits() as usize]!=0{
		    self.pc=self.regs[self.next_16_bits() as usize] as usize;
		}
		else{
		    self.pc+=1;
		}
		false
	    },
	    OpCode::Jneq => {
		if self.regs[self.next_16_bits() as usize]==0{
		    self.pc=self.regs[self.next_16_bits() as usize] as usize;
		}
		else{
		    self.pc+=1;
		}
		false
	    },
	    _ => {
		println!("Invalid opcode encountered\nleaving...");
		true
	    }
	}
    }
}
