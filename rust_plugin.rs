use vm_ffi::{FfiContext, FfiError, FfiFunction, FfiRegistry, FfiResult, FfiType, FfiValue, RawFfiFunction};

#[unsafe(no_mangle)]
pub static IONVM_PLUGIN_MAGIC: u32 = 0x494F4E56;

#[unsafe(no_mangle)]
pub static IONVM_RUST_PLUGIN: u32 = 1;

pub struct Square;

impl RawFfiFunction for Square {
    fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match args[0] {
            FfiValue::Integer(n) => Ok(FfiValue::Integer(n * n).into()),
            _ => Err(FfiError::ArgumentType {
                expected: "Integer".to_string(),
                got: format!("{:?}", args[0]),
            }),
        }
    }

    fn name(&self) -> &str {
        "square"
    }

    fn arity(&self) -> usize {
        1
    }

    fn return_type(&self) -> FfiType {
        FfiType::Int
    }
}

impl FfiFunction for Square {}

pub struct RustMathSquare;

impl RawFfiFunction for RustMathSquare {
    fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match args[0] {
            FfiValue::Integer(n) => Ok(FfiValue::Integer(n * n).into()),
            _ => Err(FfiError::ArgumentType {
                expected: "Integer".to_string(),
                got: format!("{:?}", args[0]),
            }),
        }
    }

    fn name(&self) -> &str {
        "rust_math.square"
    }

    fn arity(&self) -> usize {
        1
    }

    fn return_type(&self) -> FfiType {
        FfiType::Int
    }
}

impl FfiFunction for RustMathSquare {}

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn ionvm_rust_register(registry: &mut FfiRegistry) {
    registry.register(Square);
    registry.register(RustMathSquare);
}
