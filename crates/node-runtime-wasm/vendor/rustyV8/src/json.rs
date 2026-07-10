// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
//! A JSON Parser and Stringifier.
use crate::Context;
use crate::HandleScope;
use crate::Local;
use crate::String;
use crate::Value;

unsafe extern "C" {
  fn v8__JSON__Parse(
    context: *const Context,
    json_string: *const String,
  ) -> *const Value;
  fn v8__JSON__Stringify(
    context: *const Context,
    json_object: *const Value,
  ) -> *const String;
}

/// Tries to parse the string `json_string` and returns it as value if
/// successful.
#[inline(always)]
pub fn parse<'s>(
  scope: &mut HandleScope<'s>,
  json_string: Local<'_, String>,
) -> Option<Local<'s, Value>> {
  unsafe {
    scope
      .cast_local(|sd| v8__JSON__Parse(sd.get_current_context(), &*json_string))
  }
}

/// Tries to stringify the JSON-serializable object `json_object` and returns
/// it as string if successful.
#[inline(always)]
pub fn stringify<'s>(
  scope: &mut HandleScope<'s>,
  json_object: Local<'_, Value>,
) -> Option<Local<'s, String>> {
  unsafe {
    scope.cast_local(|sd| {
      v8__JSON__Stringify(sd.get_current_context(), &*json_object)
    })
  }
}
