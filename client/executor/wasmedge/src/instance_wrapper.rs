use sc_executor_common::error::{Result, WasmError};
use wasmedge_sys::{
    FuncType, Function, Global, ImportObject, MemType, Memory, Module, Store, Value, Vm,
};

use crate::{host::HostFunction, BoxHostFunc};
pub struct InstanceWrapper {
    store: Store,
    pub memory_ref: Memory,
    vm: Vm,
}

impl InstanceWrapper {
    pub fn new(
        module: Module,
        heap_pages: u32,
        _max_memory_size: Option<u32>,
        host_functions: &Vec<HostFunction>,
    ) -> Result<Self> {
        let mut import = ImportObject::create("env")
            .map_err(|_| WasmError::Other("ImportObj create err".to_owned()))?;

        let mem_type =
            MemType::create(heap_pages..=u32::MAX).map_err(|_| WasmError::InvalidMemory)?;
        let memory =
            wasmedge_sys::Memory::create(mem_type).map_err(|_| WasmError::InvalidMemory)?;
        import.add_memory("memory", memory);
        for func in host_functions {
            // HostFunction::wrap(name.clone(), &mut vm, ty.clone(), *func);
            println!("start regist func");
            // func.function.clone() might cause memory leak.
            import.add_func(func.name.clone(), func.function.clone());
        }
        let mut vm =
            Vm::create(None, None).map_err(|_| WasmError::Other("VM create err".to_owned()))?;

        vm.register_wasm_from_import(import)
            .map_err(|_| WasmError::Other("vm regist import err".to_owned()))?;
        vm.register_wasm_from_module("default", module)
            .map_err(|_| WasmError::Other("vm regist module err".to_owned()))?;

        let store = vm
            .store_mut()
            .map_err(|_| WasmError::Other("store err".to_owned()))?;

        let memory_ref = store
            .find_memory_registered("env", "memory")
            .map_err(|_| WasmError::Other("memory not found".to_owned()))?;
        Ok(InstanceWrapper {
            store,
            memory_ref,
            vm,
        })
    }

    pub fn extract_heap_base(&self) -> Result<u32> {
        let global = self
            .vm
            .store_mut()
            .map_err(|_| WasmError::Other("store not found in vm".to_owned()))?
            .find_global_registered("default", "__heap_base")
            .map_err(|_| WasmError::Other("global __heap_base not found".to_owned()))?;

        Ok(u32::from_le_bytes(
            global.get_value().to_i32().to_le_bytes(),
        ))
    }

    pub fn get_global(&mut self, name: &str) -> Option<Global> {
        self.store.find_global(name).ok()
    }

    pub fn call(
        &self,
        name: impl AsRef<str>,
        params: impl IntoIterator<Item = Value>,
    ) -> Result<u64> {
        let ret = self
            .vm
            .run_registered_function("default", name, params)
            .map_err(|e| WasmError::Other(e.to_string()))?;
        let ret = u64::from_le_bytes(ret[0].to_i64().to_le_bytes());
        println!("ret: {:?}", ret);
        Ok(ret)
    }
}
