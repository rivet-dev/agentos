use crate::Isolate;
use crate::Local;
use crate::Value;
use crate::binding::*;
use std::ffi::c_void;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CFunction(v8__CFunction);

impl CFunction {
  pub const fn new(address: *const c_void, type_info: &CFunctionInfo) -> Self {
    Self(v8__CFunction {
      address_: address,
      type_info_: &type_info.0,
    })
  }

  pub const fn address(&self) -> *const c_void {
    self.0.address_
  }

  pub const fn type_info(&self) -> &CFunctionInfo {
    // SAFETY: We initialize this field with a reference. and
    // the layout of CFunctionInfo is identical to v8_CFunctionInfo.
    unsafe { &*(self.0.type_info_ as *const CFunctionInfo) }
  }
}

#[repr(transparent)]
pub struct CFunctionInfo(v8__CFunctionInfo);

impl CFunctionInfo {
  /// Construct a struct to hold a CFunction's type information.
  /// |return_info| describes the function's return type.
  /// |arg_info| is an array of |arg_count| CTypeInfos describing the
  ///   arguments. Only the last argument may be of the special type
  ///   CTypeInfo::kCallbackOptionsType.
  pub const fn new(
    return_info: CTypeInfo,
    arg_info: &[CTypeInfo],
    repr: Int64Representation,
  ) -> Self {
    Self(v8__CFunctionInfo {
      arg_count_: arg_info.len() as _,
      arg_info_: arg_info.as_ptr() as _,
      repr_: repr as _,
      return_info_: return_info.0,
    })
  }
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Int64Representation {
  /// Use numbers to represent 64 bit integers.
  Number = v8_CFunctionInfo_Int64Representation_kNumber,
  /// Use BigInts to represent 64 bit integers.
  BigInt = v8_CFunctionInfo_Int64Representation_kBigInt,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CTypeInfo(v8_CTypeInfo);

impl CTypeInfo {
  pub const fn new(r#type: Type, flags: Flags) -> Self {
    Self(v8_CTypeInfo {
      flags_: flags.bits(),
      sequence_type_: v8_CTypeInfo_SequenceType_kScalar,
      type_: r#type as _,
    })
  }
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Type {
  Void = v8_CTypeInfo_Type_kVoid,
  Bool = v8_CTypeInfo_Type_kBool,
  Uint8 = v8_CTypeInfo_Type_kUint8,
  Int32 = v8_CTypeInfo_Type_kInt32,
  Uint32 = v8_CTypeInfo_Type_kUint32,
  Int64 = v8_CTypeInfo_Type_kInt64,
  Uint64 = v8_CTypeInfo_Type_kUint64,
  Float32 = v8_CTypeInfo_Type_kFloat32,
  Float64 = v8_CTypeInfo_Type_kFloat64,
  Pointer = v8_CTypeInfo_Type_kPointer,
  V8Value = v8_CTypeInfo_Type_kV8Value,
  SeqOneByteString = v8_CTypeInfo_Type_kSeqOneByteString,
  ApiObject = v8_CTypeInfo_Type_kApiObject,
  Any = v8_CTypeInfo_Type_kAny,
  CallbackOptions = 255,
}

impl Type {
  // const fn since From<T> is not const
  pub const fn as_info(self) -> CTypeInfo {
    CTypeInfo::new(self, Flags::empty())
  }
}

impl From<Type> for CTypeInfo {
  fn from(t: Type) -> Self {
    Self::new(t, Flags::empty())
  }
}

bitflags::bitflags! {
  pub struct Flags: u8 {
    /// Must be an ArrayBuffer or TypedArray
    const AllowShared = v8_CTypeInfo_Flags_kAllowSharedBit;
    /// T must be integral
    const EnforceRange = v8_CTypeInfo_Flags_kEnforceRangeBit;
    /// T must be integral
    const Clamp = v8_CTypeInfo_Flags_kClampBit;
    /// T must be float or double
    const IsRestricted = v8_CTypeInfo_Flags_kIsRestrictedBit;
  }
}

/// A struct which may be passed to a fast call callback, like so
/// ```c
/// void FastMethodWithOptions(int param, FastApiCallbackOptions& options);
/// ```
#[repr(C)]
pub struct FastApiCallbackOptions<'a> {
  pub isolate: *mut Isolate,
  /// The `data` passed to the FunctionTemplate constructor, or `undefined`.
  pub data: Local<'a, Value>,
}

pub type FastApiOneByteString = v8__FastOneByteString;

impl FastApiOneByteString {
  #[inline(always)]
  pub fn as_bytes(&self) -> &[u8] {
    // Ensure that we never create a null-ptr slice (even a zero-length null-ptr slice
    // is invalid because of Rust's niche packing).
    if self.data.is_null() {
      return &mut [];
    }

    // SAFETY: The data is guaranteed to be valid for the length of the string.
    unsafe { std::slice::from_raw_parts(self.data as _, self.length as usize) }
  }
}
