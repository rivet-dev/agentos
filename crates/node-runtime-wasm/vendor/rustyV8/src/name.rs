// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use std::num::NonZeroI32;

use crate::Name;
use crate::support::int;

unsafe extern "C" {
  fn v8__Name__GetIdentityHash(this: *const Name) -> int;
}

impl Name {
  /// Returns the V8 hash value for this value. The current implementation
  /// uses a hidden property to store the identity hash.
  ///
  /// The return value will never be 0. Also, it is not guaranteed to be
  /// unique.
  #[inline(always)]
  pub fn get_identity_hash(&self) -> NonZeroI32 {
    unsafe { NonZeroI32::new_unchecked(v8__Name__GetIdentityHash(self)) }
  }
}
