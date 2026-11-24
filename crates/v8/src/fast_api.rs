use crate::Isolate;
use crate::Local;
use crate::Value;
use crate::binding::*;
use crate::isolate::RealIsolate;
use std::ffi::c_void;
use std::ptr::NonNull;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CFunction(v8__CFunction);

impl CFunction {
  /// Construct a `CFunction` from a function address and its type info.
  ///
  /// `type_info` is borrowed for `'static` because the resulting `CFunction`
  /// stores the raw `v8::CFunctionInfo*` pointer and, once handed to V8 via
  /// [`FunctionBuilder::build_fast`], that pointer is retained for the
  /// lifetime of the `FunctionTemplate` (and copied into each fast-call
  /// dispatch site). Anything other than `'static` storage for the type info
  /// would let the pointer dangle.
  ///
  /// In practice this is achieved by constructing the `CFunctionInfo` inside
  /// a `const` initializer (where reference-to-temporary rvalues are promoted
  /// to `'static`), exactly as the existing test/bench patterns do:
  ///
  /// ```ignore
  /// const FAST_CALL: v8::fast_api::CFunction = v8::fast_api::CFunction::new(
  ///   my_fast_fn as _,
  ///   &v8::fast_api::CFunctionInfo::new(
  ///     v8::fast_api::Type::Void.as_info(),
  ///     &[v8::fast_api::Type::V8Value.as_info()],
  ///     v8::fast_api::Int64Representation::Number,
  ///   ),
  /// );
  /// ```
  ///
  /// [`FunctionBuilder::build_fast`]: crate::FunctionBuilder::build_fast
  pub const fn new(
    address: *const c_void,
    type_info: &'static CFunctionInfo,
  ) -> Self {
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
  ///
  /// `arg_info` is borrowed for `'static` because the resulting
  /// `CFunctionInfo` stores the raw `arg_info.as_ptr()` pointer and V8 reads
  /// it on every fast-call invocation through the `CFunction` chain. As with
  /// [`CFunction::new`], `const` initializers make this trivial: rvalues
  /// such as `&[Type::V8Value.as_info()]` are promoted to `'static` storage.
  pub const fn new(
    return_info: CTypeInfo,
    arg_info: &'static [CTypeInfo],
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
      type_: r#type as _,
      flags_: flags.bits(),
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
  pub(crate) isolate: *mut RealIsolate,
  /// The `data` passed to the FunctionTemplate constructor, or `undefined`.
  pub data: Local<'a, Value>,
}

impl<'a> FastApiCallbackOptions<'a> {
  pub unsafe fn isolate_unchecked(&self) -> &'a Isolate {
    unsafe {
      Isolate::from_raw_ref(std::mem::transmute::<
        &*mut RealIsolate,
        &NonNull<RealIsolate>,
      >(&self.isolate))
    }
  }

  pub unsafe fn isolate_unchecked_mut(&mut self) -> &mut Isolate {
    unsafe {
      Isolate::from_raw_ref_mut(std::mem::transmute::<
        &mut *mut RealIsolate,
        &mut NonNull<RealIsolate>,
      >(&mut self.isolate))
    }
  }
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
