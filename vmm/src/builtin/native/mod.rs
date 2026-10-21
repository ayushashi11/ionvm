pub mod c_abi;
pub mod fiber;
pub mod loader;
pub mod rust_lib;

pub use fiber::ion_c_yield;

pub use c_abi::{
    IonCData, IonCFunction, IonCFunctionPtr, IonCPluginInitFn, IonCRegisterCallback, IonCValue,
    ION_VAL_BOOL, ION_VAL_FLOAT, ION_VAL_INT, ION_VAL_NIL, ION_VAL_POINTER, ION_VAL_STRING,
};
pub use loader::{
    get_native_version, get_registered_c_functions, load_native_library, register_rust_plugin,
    sync_registered_natives, IONVM_PLUGIN_MAGIC,
};
pub use rust_lib::RustPluginRegisterFn;
