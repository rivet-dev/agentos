/// A WebAssembly value crossing the agentOS host-import boundary. The ABI
/// uses only 32- and 64-bit integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Val {
    I32(i32),
    I64(i64),
}

impl Val {
    /// The zero value of an ABI type, used to size a result slot.
    pub fn zero(ty: crate::abi::CoreValueType) -> Self {
        match ty {
            crate::abi::CoreValueType::I32 => Val::I32(0),
            crate::abi::CoreValueType::I64 => Val::I64(0),
        }
    }
}
