// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::ArrayBuffer;
use crate::HandleScope;
use crate::Local;
use crate::TypedArray;
use crate::binding::v8__TypedArray__kMaxByteLength;
use crate::support::size_t;
use paste::paste;

unsafe extern "C" {
  fn v8__TypedArray__Length(this: *const TypedArray) -> size_t;
}

impl TypedArray {
  /// The largest supported typed array byte size. Each subclass defines a
  /// type-specific max_length for the maximum length that can be passed to new.
  pub const MAX_BYTE_LENGTH: usize = v8__TypedArray__kMaxByteLength;

  /// Number of elements in this typed array
  /// (e.g. for Int16Array, |ByteLength|/2).
  #[inline(always)]
  pub fn length(&self) -> usize {
    unsafe { v8__TypedArray__Length(self) }
  }
}

macro_rules! typed_array {
  ($name:ident) => {
    paste! {
      use crate::$name;

      unsafe extern "C" {
        fn [< v8__ $name __New >](
          buf_ptr: *const ArrayBuffer,
          byte_offset: usize,
          length: usize,
        ) -> *const $name;
      }

      impl $name {
        #[inline(always)]
        pub fn new<'s>(
          scope: &mut HandleScope<'s>,
          buf: Local<ArrayBuffer>,
          byte_offset: usize,
          length: usize,
        ) -> Option<Local<'s, $name>> {
          unsafe { scope.cast_local(|_| [< v8__ $name __New >](&*buf, byte_offset, length)) }
        }

        #[doc = concat!("The largest ", stringify!($name), " size that can be constructed using `new`.")]
        pub const MAX_LENGTH: usize = crate::binding::[< v8__ $name __kMaxLength >];
      }
    }
  };
}

typed_array!(Uint8Array);
typed_array!(Uint8ClampedArray);
typed_array!(Int8Array);
typed_array!(Uint16Array);
typed_array!(Int16Array);
typed_array!(Uint32Array);
typed_array!(Int32Array);
typed_array!(Float32Array);
typed_array!(Float64Array);
typed_array!(BigUint64Array);
typed_array!(BigInt64Array);
