mod host;
mod instance_wrapper;
mod runtime;
mod util;

#[cfg(test)]
mod tests;

use host::Caller;

pub use crate::runtime::create_runtime;
pub use crate::runtime::Config;
pub type BoxHostFunc = Box<
	dyn Fn(Caller, Vec<wasmedge_sys::Value>) -> std::result::Result<Vec<wasmedge_sys::Value>, u8>,
>;
