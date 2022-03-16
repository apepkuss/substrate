use std::sync::Arc;

use sc_allocator::FreeingBumpHeapAllocator;
use sc_executor_common::error::{Result, WasmError};
use sc_executor_common::runtime_blob::RuntimeBlob;
use sc_executor_common::runtime_blob::{self, ExposedMutableGlobalsSet};
use sc_executor_common::runtime_blob::{DataSegmentsSnapshot, GlobalsSnapshot};
use sc_executor_common::wasm_runtime::{InvokeMethod, WasmInstance, WasmModule};
use sp_runtime_interface::unpack_ptr_and_len;
use sp_wasm_interface::{Pointer, Value, WordSize};
use wasmedge_sys::{FuncType, Loader, Memory};

use crate::host::{Caller, HostFunction};
use crate::instance_wrapper::InstanceWrapper;
use crate::{util, BoxHostFunc};
struct InstanceGlobals<'a> {
	instance: &'a mut InstanceWrapper,
}

impl<'a> runtime_blob::InstanceGlobals for InstanceGlobals<'a> {
	type Global = wasmedge_sys::Global;

	fn get_global(&mut self, export_name: &str) -> Self::Global {
		self.instance
			.get_global(export_name)
			.expect("get_global is guaranteed to be called with an export name of a global; qed")
	}

	fn get_global_value(&mut self, global: &Self::Global) -> Value {
		util::from_wasmedge_val(global.get_value())
	}

	fn set_global_value(&mut self, global: &Self::Global, value: Value) {
		let mut global = global.clone();
		global.set_value(util::into_wasmedge_val(value)).expect(
			"the value is guaranteed to be of the same value; the global is guaranteed to be mutable; qed"
		);
	}
}

/// 用来保存实例最初始的状态，每次call的时候都把实例恢复到最初始的状态
struct InstanceSnapshotData {
	mutable_globals: ExposedMutableGlobalsSet,
	data_segments_snapshot: Arc<DataSegmentsSnapshot>,
}

unsafe impl Send for WasmEdgeRuntime {}
unsafe impl Sync for WasmEdgeRuntime {}
pub struct WasmEdgeRuntime {
	module: Vec<u8>,
	snapshot_data: InstanceSnapshotData,
	host_functions: Vec<HostFunction>,
	config: Config,
}

impl WasmModule for WasmEdgeRuntime {
	fn new_instance(&self) -> Result<Box<dyn WasmInstance>> {
		let loader =
			Loader::create(None).map_err(|_| WasmError::Other("loader create err".to_owned()))?;
		let module = loader.from_buffer(&self.module).map_err(|_| WasmError::InvalidModule)?;

		let mut instance_wrapper = InstanceWrapper::new(
			module,
			self.config.heap_pages,
			self.config.max_memory_size,
			&self.host_functions,
		)?;
		let heap_base = instance_wrapper.extract_heap_base()?;

		//从snapshot_data.mutable_globals中提取出，全局变量的初始值保存起来
		let globals_snapshot = GlobalsSnapshot::take(
			&self.snapshot_data.mutable_globals,
			&mut InstanceGlobals { instance: &mut instance_wrapper },
		);

		Ok(Box::new(WasmEdgeInstance {
			instance: instance_wrapper,
			heap_base,
			globals_snapshot,
			data_segments_snapshot: self.snapshot_data.data_segments_snapshot.clone(),
		}))
	}
}
pub struct Config {
	// 运行时实例初始化后的，为memory分配的初始page大小
	pub heap_pages: u32,
	// 后续memory允许增加到的最大bytes
	pub max_memory_size: Option<u32>,
}

pub fn create_runtime(
	blob: RuntimeBlob,
	config: Config,
	host_functions: Vec<HostFunction>,
) -> Result<WasmEdgeRuntime> {
	let data_segments_snapshot =
		DataSegmentsSnapshot::take(&blob).map_err(|e| WasmError::Other(e.to_string()))?;
	let data_segments_snapshot = Arc::new(data_segments_snapshot);

	let mutable_globals = ExposedMutableGlobalsSet::collect(&blob);
	let snapshot_data = InstanceSnapshotData { mutable_globals, data_segments_snapshot };

	Ok(WasmEdgeRuntime { module: blob.serialize(), snapshot_data, host_functions, config })
}

unsafe impl Send for WasmEdgeInstance {}
unsafe impl Sync for WasmEdgeInstance {}
pub struct WasmEdgeInstance {
	instance: InstanceWrapper,
	heap_base: u32,
	globals_snapshot: GlobalsSnapshot<wasmedge_sys::Global>,
	data_segments_snapshot: Arc<DataSegmentsSnapshot>,
}

impl WasmInstance for WasmEdgeInstance {
	fn call(&mut self, method: InvokeMethod, data: &[u8]) -> Result<Vec<u8>> {
		self.data_segments_snapshot.apply(|ptr, data| {
			self.instance
				.memory_ref
				.set_data(data.iter().cloned(), ptr)
				.map_err(|_| WasmError::InvalidMemory)
		})?;

		self.globals_snapshot
			.apply(&mut InstanceGlobals { instance: &mut self.instance });

		let mut allocator = FreeingBumpHeapAllocator::new(self.heap_base);

		let (data_ptr, data_len) =
			inject_input_data(&mut self.instance, &mut allocator, data).unwrap();

		let params = vec![
			wasmedge_sys::Value::from_i32(i32::from_le_bytes(u32::from(data_ptr).to_le_bytes())),
			wasmedge_sys::Value::from_i32(i32::from_le_bytes(data_len.to_le_bytes())),
		];

		//instance.call传入函数名，以及函数需要的数据的指针和数据长度
		//返回一个指针，以及数据的长度
		let ret = match method {
			InvokeMethod::Export(name) => self.instance.call(name, params).map(unpack_ptr_and_len),
			_ => unimplemented!(),
		};
		let (output_ptr, output_len) = ret?;
		println!("output_len: {:?}", output_len);

		//根据函数返回的指针和数据长度，从memory中找到对应的数据
		let res: Vec<u8> = self
			.instance
			.memory_ref
			.get_data(output_ptr, output_len)
			.map_err(|_| WasmError::InvalidMemory)?;

		Ok(res)
	}

	fn get_global_const(&mut self, _name: &str) -> Result<Option<Value>> {
		unimplemented!()
	}
}

struct MemoryWrapper<'a> {
	memory: &'a mut Memory,
}

impl<'a> sc_allocator::Memory for MemoryWrapper<'a> {
	fn read_le_u64(&self, ptr: u32) -> std::result::Result<u64, sc_allocator::Error> {
		let data: Vec<u8> = self
			.memory
			.get_data(ptr, 8)
			.map_err(|_| sc_allocator::Error::Other("memory out of range"))?;
		let mut res: [u8; 8] = [0; 8];
		res.copy_from_slice(&data);
		let res = u64::from_le_bytes(res);
		Ok(res)
	}

	fn write_le_u64(&mut self, ptr: u32, val: u64) -> std::result::Result<(), sc_allocator::Error> {
		self.memory
			.set_data(val.to_le_bytes(), ptr)
			.map_err(|_| sc_allocator::Error::AllocatorOutOfSpace)?;
		Ok(())
	}

	fn size(&self) -> u32 {
		u32::MAX
	}
}

fn inject_input_data(
	instance: &mut InstanceWrapper,
	allocator: &mut FreeingBumpHeapAllocator,
	data: &[u8],
) -> Result<(Pointer<u8>, WordSize)> {
	let data_len = data.len() as WordSize;
	let data_ptr =
		allocator.allocate(&mut MemoryWrapper { memory: &mut instance.memory_ref }, data_len)?;
	util::write_memory_from(&mut instance.memory_ref, data_ptr.into(), data)?;
	Ok((data_ptr, data_len))
}

// fn extract_output_data(
// 	instance: &InstanceWrapper,
// 	output_ptr: u32,
// 	output_len: u32,
// ) -> Result<Vec<u8>> {
// 	let mut output = vec![0; output_len as usize];
// 	util::read_memory_into(instance.store(), Pointer::new(output_ptr), &mut output)?;
// 	Ok(output)
// }
