use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::null;

use crate::Context;
use crate::Data;
use crate::HandleScope;
use crate::Local;
use crate::Script;
use crate::String;
use crate::UnboundScript;
use crate::Value;

/// The origin, within a file, of a script.
#[repr(C)]
#[derive(Debug)]
pub struct ScriptOrigin<'s>(
  [u8; crate::binding::v8__ScriptOrigin_SIZE],
  PhantomData<&'s ()>,
);

unsafe extern "C" {
  fn v8__Script__Compile(
    context: *const Context,
    source: *const String,
    origin: *const ScriptOrigin,
  ) -> *const Script;
  fn v8__Script__GetUnboundScript(
    script: *const Script,
  ) -> *const UnboundScript;
  fn v8__Script__Run(
    script: *const Script,
    context: *const Context,
  ) -> *const Value;

  fn v8__ScriptOrigin__CONSTRUCT(
    buf: *mut MaybeUninit<ScriptOrigin>,
    resource_name: *const Value,
    resource_line_offset: i32,
    resource_column_offset: i32,
    resource_is_shared_cross_origin: bool,
    script_id: i32,
    source_map_url: *const Value,
    resource_is_opaque: bool,
    is_wasm: bool,
    is_module: bool,
    host_defined_options: *const Data,
  );
  fn v8__ScriptOrigin__ScriptId(origin: *const ScriptOrigin) -> i32;
  fn v8__ScriptOrigin__ResourceName(
    origin: *const ScriptOrigin,
  ) -> *const Value;
  fn v8__ScriptOrigin__SourceMapUrl(
    origin: *const ScriptOrigin,
  ) -> *const Value;
}

impl Script {
  /// A shorthand for ScriptCompiler::Compile().
  #[inline(always)]
  pub fn compile<'s>(
    scope: &mut HandleScope<'s>,
    source: Local<String>,
    origin: Option<&ScriptOrigin>,
  ) -> Option<Local<'s, Script>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Script__Compile(
          sd.get_current_context(),
          &*source,
          origin.map_or_else(null, |r| r as *const _),
        )
      })
    }
  }

  /// Returns the corresponding context-unbound script.
  #[inline(always)]
  pub fn get_unbound_script<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Local<'s, UnboundScript> {
    unsafe {
      scope
        .cast_local(|_| v8__Script__GetUnboundScript(self))
        .unwrap()
    }
  }

  /// Runs the script returning the resulting value. It will be run in the
  /// context in which it was created (ScriptCompiler::CompileBound or
  /// UnboundScript::BindToCurrentContext()).
  #[inline]
  pub fn run<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| v8__Script__Run(self, sd.get_current_context()))
    }
  }
}

/// The origin, within a file, of a script.
impl<'s> ScriptOrigin<'s> {
  #[allow(clippy::too_many_arguments)]
  #[inline(always)]
  pub fn new(
    // TODO(littledivy): remove
    _scope: &mut HandleScope<'s, ()>,
    resource_name: Local<'s, Value>,
    resource_line_offset: i32,
    resource_column_offset: i32,
    resource_is_shared_cross_origin: bool,
    script_id: i32,
    source_map_url: Option<Local<'s, Value>>,
    resource_is_opaque: bool,
    is_wasm: bool,
    is_module: bool,
    host_defined_options: Option<Local<'s, Data>>,
  ) -> Self {
    unsafe {
      let mut buf = std::mem::MaybeUninit::<ScriptOrigin>::uninit();
      v8__ScriptOrigin__CONSTRUCT(
        &mut buf,
        &*resource_name,
        resource_line_offset,
        resource_column_offset,
        resource_is_shared_cross_origin,
        script_id,
        source_map_url.map_or_else(null, |l| &*l as *const Value),
        resource_is_opaque,
        is_wasm,
        is_module,
        host_defined_options.map_or_else(null, |l| &*l as *const Data),
      );
      buf.assume_init()
    }
  }

  #[inline(always)]
  pub fn script_id(&self) -> i32 {
    unsafe { v8__ScriptOrigin__ScriptId(self as *const _) }
  }

  #[inline(always)]
  pub fn resource_name(&self) -> Option<Local<'s, Value>> {
    unsafe {
      let ptr = v8__ScriptOrigin__ResourceName(self);
      Local::from_raw(ptr)
    }
  }

  #[inline(always)]
  pub fn source_map_url(&self) -> Option<Local<'s, Value>> {
    unsafe {
      let ptr = v8__ScriptOrigin__SourceMapUrl(self);
      Local::from_raw(ptr)
    }
  }
}
