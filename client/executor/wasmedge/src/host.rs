use wasmedge_sys::{FuncType, Function, Memory, Store, Value, Vm};

#[derive(Clone)]
pub struct HostFunction {
	pub function: Function,
	pub name: String,
}

pub struct Caller<'a> {
	store: &'a Store,
}

impl HostFunction {
	pub fn wrap(
		name: String,
		vm: &mut Vm,
		func_ty: FuncType,
		func: Box<dyn Fn(Caller, Vec<Value>) -> Result<Vec<Value>, u8>>,
	) -> Self {
		let store = vm.store_mut().unwrap();
		let real_fn = Box::new(move |params: Vec<Value>| {
			let res = func(Caller { store: &store }, params);
			res
		});

		let function = Function::create(func_ty, real_fn, 0).unwrap();
		HostFunction { function, name }
	}
}
