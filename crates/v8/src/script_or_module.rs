// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::Data;
use crate::Local;
use crate::ScriptOrModule;
use crate::Value;

unsafe extern "C" {
  fn v8__ScriptOrModule__GetResourceName(
    this: *const ScriptOrModule,
  ) -> *const Value;

  fn v8__ScriptOrModule__HostDefinedOptions(
    this: *const ScriptOrModule,
  ) -> *const Data;
}

impl ScriptOrModule {
  /// The name that was passed by the embedder as ResourceName to the
  /// ScriptOrigin. This can be either a v8::String or v8::Undefined.
  #[inline(always)]
  pub fn get_resource_name(&self) -> Local<'_, Value> {
    // Note: the C++ `v8::ScriptOrModule::GetResourceName()` does not actually
    // return a local handle, but rather a handle whose lifetime is bound to
    // the related `ScriptOrModule` object.
    unsafe {
      let ptr = v8__ScriptOrModule__GetResourceName(self);
      Local::from_raw(ptr).unwrap()
    }
  }

  /// The options that were passed by the embedder as HostDefinedOptions to the
  /// ScriptOrigin.
  #[inline(always)]
  pub fn host_defined_options(&self) -> Local<'_, Data> {
    // Note: the C++ `v8::ScriptOrModule::HostDefinedOptions()` does not
    // actually return a local handle, but rather a handle whose lifetime is
    // bound to the related `ScriptOrModule` object.
    unsafe {
      let ptr = v8__ScriptOrModule__HostDefinedOptions(self);
      Local::from_raw(ptr).unwrap()
    }
  }
}
