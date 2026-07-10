use crate::CachedData;
use crate::HandleScope;
use crate::Local;
use crate::UnboundModuleScript;
use crate::UniqueRef;
use crate::Value;

unsafe extern "C" {
  fn v8__UnboundModuleScript__CreateCodeCache(
    script: *const UnboundModuleScript,
  ) -> *mut CachedData<'static>;

  fn v8__UnboundModuleScript__GetSourceMappingURL(
    script: *const UnboundModuleScript,
  ) -> *const Value;

  fn v8__UnboundModuleScript__GetSourceURL(
    script: *const UnboundModuleScript,
  ) -> *const Value;
}

impl UnboundModuleScript {
  /// Creates and returns code cache for the specified unbound_module_script.
  /// This will return nullptr if the script cannot be serialized. The
  /// CachedData returned by this function should be owned by the caller.
  #[inline(always)]
  pub fn create_code_cache(&self) -> Option<UniqueRef<CachedData<'static>>> {
    let code_cache = unsafe {
      UniqueRef::try_from_raw(v8__UnboundModuleScript__CreateCodeCache(self))
    };
    if let Some(code_cache) = &code_cache {
      debug_assert_eq!(
        code_cache.buffer_policy(),
        crate::script_compiler::BufferPolicy::BufferOwned
      );
    }
    code_cache
  }

  pub fn get_source_mapping_url<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Local<'s, Value> {
    unsafe {
      scope
        .cast_local(|_| v8__UnboundModuleScript__GetSourceMappingURL(self))
        .unwrap()
    }
  }

  pub fn get_source_url<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Local<'s, Value> {
    unsafe {
      scope
        .cast_local(|_| v8__UnboundModuleScript__GetSourceURL(self))
        .unwrap()
    }
  }
}
