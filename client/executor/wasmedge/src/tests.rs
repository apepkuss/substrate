use crate::{
	host::{Caller, HostFunction},
	Config,
};
use sc_executor_common::{
	runtime_blob::RuntimeBlob,
	wasm_runtime::{InvokeMethod, WasmModule},
};
use std::sync::Arc;
use wasmedge_sys::{FuncType, Function, ImportObject, Loader, ValType, Value, Vm};
use wat;

struct RuntimeBuilder {
	code: Option<&'static str>,
	heap_pages: u32,
	_max_memory_size: Option<usize>,
}

impl RuntimeBuilder {
	fn new_on_demand() -> Self {
		Self { code: None, heap_pages: 2048, _max_memory_size: None }
	}

	fn use_wat(&mut self, code: &'static str) {
		self.code = Some(code);
	}

	fn _max_memory_size(&mut self, max_memory_size: Option<usize>) {
		self._max_memory_size = max_memory_size;
	}

	fn build(self) -> Arc<dyn WasmModule> {
		let blob = {
			let wasm: Vec<u8>;

			let wasm = match self.code {
				None => unimplemented!(),
				Some(wat) => {
					wasm = wat::parse_str(wat).expect("wat parsing failed");
					&wasm
				}
			};

			RuntimeBlob::uncompress_if_needed(&wasm)
				.expect("failed to create a runtime blob out of test runtime")
		};
		let func_ty = FuncType::create(vec![ValType::I32; 1], vec![ValType::I32; 1])
			.expect("failed to create a FuncType");
		let func =
			Function::create(func_ty, Box::new(add_one), 0).expect("fail to create a Function");
		let func = HostFunction { name: "add_one".to_owned(), function: func };

		let rt = crate::create_runtime(
			blob,
			Config { heap_pages: self.heap_pages, max_memory_size: None },
			vec![func],
		)
		.expect("runtime build err");
		Arc::new(rt)
	}
}
//inputs: ptr -> memory
//memory?
fn add_one(inputs: Vec<Value>) -> Result<Vec<Value>, u8> {
	if inputs.len() != 1 {
		return Err(1);
	}
	let v = if inputs[0].ty() == ValType::I32 { inputs[0].to_i32() } else { return Err(2) };

	let res = v + 1;
	let res = Value::from_i32(res);
	Ok(vec![res])
}

#[test]
fn test_host_add_one() {
	let mut builder = RuntimeBuilder::new_on_demand();

	builder.use_wat(
		r#"
    (module
		(type $t1 (func (param i32) (result i32)))
		(import "env" "add_one" (func $add_one (type $t1)))
		(import "env" "memory" (memory 2048))
		(global (export "__heap_base") i32 (i32.const 20000))
		(export "main" (func $main))
		(func $main (param $x i32) (param $y i32) (result i64)
			(i32.store
				(i32.const 20000)
				(call $add_one	
					(i32.load (local.get $x))))
			(i32.store
				(i32.const 20100)
				(i32.const 20000))
			(i32.store
				(i32.const 20104)
				(i32.const 4))
			(i64.load (i32.const 20100))
		)
    )
    "#,
	);

	let rt = builder.build();

	let mut instance = rt.new_instance().expect("instance create failed");

	let res = instance
		.call(InvokeMethod::Export("main"), &10u32.to_le_bytes())
		.expect("instance call failed");
	assert_eq!(4, res.len());

	let mut temp = [0u8; 4];
	temp.copy_from_slice(&res);
	let val = u32::from_le_bytes(temp);
	println!("val = {:?}", val);
	assert_eq!(11, val);
}

#[test]
fn test_store_load() {
	let mut builder = RuntimeBuilder::new_on_demand();

	builder.use_wat(
		r#"
    (module
		(import "env" "memory" (memory 1024))
		(global (export "__heap_base") i32 (i32.const 0))
		(export "main" (func $main))
		(func $main (param $x i32) (param $y i32) (result i64)
			(i32.store
				(i32.const 200)
				(i32.const 66666))
			(i32.store
				(i32.const 100)
				(i32.const 200))
			(i32.store
				(i32.const 104)
				(i32.const 4))
			(i64.load (i32.const 100))
		)
    )
    "#,
	);

	let rt = builder.build();

	let mut instance = rt.new_instance().unwrap();

	let mut data: Vec<u8> = vec![];
	data.extend_from_slice(&10_i32.to_le_bytes());
	let res = instance.call(InvokeMethod::Export("main"), &data).unwrap();
	assert_eq!(4, res.len());

	let mut temp = [0u8; 4];
	temp.copy_from_slice(&res);
	let val = u32::from_le_bytes(temp);
	assert_eq!(66666, val);
}

#[test]
fn test_add_one() {
	let mut builder = RuntimeBuilder::new_on_demand();

	builder.use_wat(
		r#"
    (module
		(import "env" "memory" (memory 1024))
		(global (export "__heap_base") i32 (i32.const 0))
		(export "main" (func $main))
		(func $main (param $x i32) (param $y i32) (result i64)
			(i32.store
				(i32.const 200)
				(i32.add	
					(i32.load (local.get $x))
					(i32.const 1)
				)
			)
			(i32.store
				(i32.const 100)
				(i32.const 200))
			(i32.store
				(i32.const 104)
				(i32.const 4))
			(i64.load (i32.const 100))	
		)
    )
    "#,
	);

	let rt = builder.build();

	let mut instance = rt.new_instance().unwrap();

	let res = instance.call(InvokeMethod::Export("main"), &10u32.to_le_bytes()).unwrap();
	assert_eq!(4, res.len());

	let mut temp = [0u8; 4];
	temp.copy_from_slice(&res);
	let val = u32::from_le_bytes(temp);
	assert_eq!(11, val);
}

// #[test]
// fn test_origin() {
// 	let code = r#"
// 		(module
// 			(export "main" (func $main))
// 			(func $main (param $x i32) (param $y i32) (result i64)
// 				(i64.const 0)
// 			)
// 		)
// 		"#;
// 	let wasm: Vec<u8> = wat::parse_str(code).expect("wat parsing failed");

// 	let loader = Loader::create(None).expect("loader create failed");
// 	let module = loader.from_buffer(wasm).expect("load module failed");

// 	let mut vm = Vm::create(None, None).expect("vm vreate failed");

// 	let func_ty = FuncType::create(vec![ValType::I32; 1], vec![ValType::I32; 1])
// 		.expect("failed to create a FuncType");
// 	let func = Function::create(func_ty, Box::new(add_one), 0).expect("fail to create a Function");
// 	let func = HostFunction { name: "add_one".to_owned(), function: func };

// 	let mut import_obj = ImportObject::create("env").expect("import_obj create failed");
// 	import_obj.add_func("add_one", func.function);

// 	vm.register_wasm_from_import(import_obj).expect("vm regist import_obj failed");

// 	vm.register_wasm_from_module("default", module).expect("regist module failed");
// }
