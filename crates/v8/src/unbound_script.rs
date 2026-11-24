use crate::CachedData;
use crate::Local;
use crate::PinScope;
use crate::Script;
use crate::UnboundScript;
use crate::UniqueRef;
use crate::Value;

unsafe extern "C" {
  fn v8__UnboundScript__BindToCurrentContext(
    script: *const UnboundScript,
  ) -> *const Script;
  fn v8__UnboundScript__ScriptId(script: *const UnboundScript) -> i32;
  fn v8__UnboundScript__CreateCodeCache(
    script: *const UnboundScript,
  ) -> *mut CachedData<'static>;

  fn v8__UnboundScript__GetSourceMappingURL(
    script: *const UnboundScript,
  ) -> *const Value;

  fn v8__UnboundScript__GetSourceURL(
    script: *const UnboundScript,
  ) -> *const Value;
}

impl UnboundScript {
  /// Binds the script to the currently entered context.
  #[inline(always)]
  pub fn bind_to_current_context<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Local<'s, Script> {
    unsafe {
      scope.cast_local(|_| v8__UnboundScript__BindToCurrentContext(self))
    }
    .unwrap()
  }

  /// Returns the ID assigned to this context-unbound script by V8.
  #[inline(always)]
  pub fn script_id(&self) -> i32 {
    unsafe { v8__UnboundScript__ScriptId(self) }
  }

  /// Creates and returns code cache for the specified unbound_script.
  /// This will return nullptr if the script cannot be serialized. The
  /// CachedData returned by this function should be owned by the caller.
  #[inline(always)]
  pub fn create_code_cache(&self) -> Option<UniqueRef<CachedData<'static>>> {
    let code_cache = unsafe {
      UniqueRef::try_from_raw(v8__UnboundScript__CreateCodeCache(self))
    };
    if let Some(code_cache) = &code_cache {
      debug_assert_eq!(
        code_cache.buffer_policy(),
        crate::script_compiler::BufferPolicy::BufferOwned
      );
    }
    code_cache
  }

  #[inline(always)]
  pub fn get_source_mapping_url<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Local<'s, Value> {
    unsafe {
      scope
        .cast_local(|_| v8__UnboundScript__GetSourceMappingURL(self))
        .unwrap()
    }
  }

  #[inline(always)]
  pub fn get_source_url<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Local<'s, Value> {
    unsafe {
      scope
        .cast_local(|_| v8__UnboundScript__GetSourceURL(self))
        .unwrap()
    }
  }
}
