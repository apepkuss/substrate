use sc_executor_common::error::Result;
use sc_executor_common::error::WasmError;
use sp_wasm_interface::Value;
use wasmedge_sys::Memory;

pub fn from_wasmedge_val(val: wasmedge_sys::Value) -> Value {
	match val.ty() {
		wasmedge_sys::ValType::F32 => Value::F32(val.to_f32() as u32),
		wasmedge_sys::ValType::F64 => Value::F64(val.to_f64() as u64),
		wasmedge_sys::ValType::I32 => Value::I32(val.to_i32()),
		wasmedge_sys::ValType::I64 => Value::I64(val.to_i64()),
		v => panic!("Given value type is unsupported by Substrate: {:?}", v),
	}
}

pub fn into_wasmedge_val(val: Value) -> wasmedge_sys::Value {
	match val {
		Value::F32(n) => wasmedge_sys::Value::from_f32(n as f32),
		Value::F64(n) => wasmedge_sys::Value::from_f64(n as f64),
		Value::I32(n) => wasmedge_sys::Value::from_i32(n),
		Value::I64(n) => wasmedge_sys::Value::from_i64(n),
	}
}

pub fn write_memory_from(memory: &mut Memory, data_ptr: u32, data: &[u8]) -> Result<()> {
	memory
		.set_data(data.iter().cloned(), data_ptr)
		.map_err(|_| WasmError::InvalidMemory)?;
	Ok(())
}
