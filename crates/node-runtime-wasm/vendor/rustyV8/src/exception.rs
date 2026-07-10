#![allow(non_snake_case)]

use std::convert::TryInto;

use crate::Context;
use crate::HandleScope;
use crate::Local;
use crate::Message;
use crate::StackFrame;
use crate::StackTrace;
use crate::String;
use crate::Value;
use crate::isolate::Isolate;
use crate::support::int;

unsafe extern "C" {
  fn v8__Message__Get(this: *const Message) -> *const String;
  fn v8__Message__GetSourceLine(
    this: *const Message,
    context: *const Context,
  ) -> *const String;
  fn v8__Message__GetScriptResourceName(this: *const Message) -> *const Value;
  fn v8__Message__GetLineNumber(
    this: *const Message,
    context: *const Context,
  ) -> int;
  fn v8__Message__GetStartPosition(this: *const Message) -> int;
  fn v8__Message__GetEndPosition(this: *const Message) -> int;
  fn v8__Message__GetWasmFunctionIndex(this: *const Message) -> int;
  fn v8__Message__ErrorLevel(this: *const Message) -> int;
  fn v8__Message__GetStartColumn(this: *const Message) -> int;
  fn v8__Message__GetEndColumn(this: *const Message) -> int;
  fn v8__Message__IsSharedCrossOrigin(this: *const Message) -> bool;
  fn v8__Message__IsOpaque(this: *const Message) -> bool;
  fn v8__Message__GetStackTrace(this: *const Message) -> *const StackTrace;

  fn v8__StackTrace__CurrentStackTrace(
    isolate: *mut Isolate,
    frame_limit: int,
  ) -> *const StackTrace;
  fn v8__StackTrace__CurrentScriptNameOrSourceURL(
    isolate: *mut Isolate,
  ) -> *const String;
  fn v8__StackTrace__GetFrameCount(this: *const StackTrace) -> int;
  fn v8__StackTrace__GetFrame(
    this: *const StackTrace,
    isolate: *mut Isolate,
    index: u32,
  ) -> *const StackFrame;

  fn v8__StackFrame__GetLineNumber(this: *const StackFrame) -> int;
  fn v8__StackFrame__GetColumn(this: *const StackFrame) -> int;
  fn v8__StackFrame__GetScriptId(this: *const StackFrame) -> int;
  fn v8__StackFrame__GetScriptName(this: *const StackFrame) -> *const String;
  fn v8__StackFrame__GetScriptNameOrSourceURL(
    this: *const StackFrame,
  ) -> *const String;
  fn v8__StackFrame__GetFunctionName(this: *const StackFrame) -> *const String;
  fn v8__StackFrame__IsEval(this: *const StackFrame) -> bool;
  fn v8__StackFrame__IsConstructor(this: *const StackFrame) -> bool;
  fn v8__StackFrame__IsWasm(this: *const StackFrame) -> bool;
  fn v8__StackFrame__IsUserJavaScript(this: *const StackFrame) -> bool;

  fn v8__Exception__Error(message: *const String) -> *const Value;
  fn v8__Exception__RangeError(message: *const String) -> *const Value;
  fn v8__Exception__ReferenceError(message: *const String) -> *const Value;
  fn v8__Exception__SyntaxError(message: *const String) -> *const Value;
  fn v8__Exception__TypeError(message: *const String) -> *const Value;

  fn v8__Exception__CreateMessage(
    isolate: *mut Isolate,
    exception: *const Value,
  ) -> *const Message;
  fn v8__Exception__GetStackTrace(exception: *const Value)
  -> *const StackTrace;
}

impl StackTrace {
  /// Grab a snapshot of the current JavaScript execution stack.
  #[inline(always)]
  pub fn current_stack_trace<'s>(
    scope: &mut HandleScope<'s>,
    frame_limit: usize,
  ) -> Option<Local<'s, StackTrace>> {
    let frame_limit = frame_limit.try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__StackTrace__CurrentStackTrace(sd.get_isolate_ptr(), frame_limit)
      })
    }
  }

  /// Returns the first valid script name or source URL starting at the top of
  /// the JS stack. The returned string is either an empty handle if no script
  /// name/url was found or a non-zero-length string.
  ///
  /// This method is equivalent to calling StackTrace::CurrentStackTrace and
  /// walking the resulting frames from the beginning until a non-empty script
  /// name/url is found. The difference is that this method won't allocate
  /// a stack trace.
  ///
  #[inline(always)]
  pub fn current_script_name_or_source_url<'s>(
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__StackTrace__CurrentScriptNameOrSourceURL(sd.get_isolate_ptr())
      })
    }
  }

  /// Returns the number of StackFrames.
  #[inline(always)]
  pub fn get_frame_count(&self) -> usize {
    unsafe { v8__StackTrace__GetFrameCount(self) as usize }
  }

  /// Returns a StackFrame at a particular index.
  #[inline(always)]
  pub fn get_frame<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    index: usize,
  ) -> Option<Local<'s, StackFrame>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__StackTrace__GetFrame(self, sd.get_isolate_ptr(), index as u32)
      })
    }
  }
}

impl StackFrame {
  /// Returns the number, 1-based, of the line for the associated function call.
  /// This method will return Message::kNoLineNumberInfo if it is unable to
  /// retrieve the line number, or if kLineNumber was not passed as an option
  /// when capturing the StackTrace.
  #[inline(always)]
  pub fn get_line_number(&self) -> usize {
    unsafe { v8__StackFrame__GetLineNumber(self) as usize }
  }

  /// Returns the 1-based column offset on the line for the associated function
  /// call.
  /// This method will return Message::kNoColumnInfo if it is unable to retrieve
  /// the column number, or if kColumnOffset was not passed as an option when
  /// capturing the StackTrace.
  #[inline(always)]
  pub fn get_column(&self) -> usize {
    unsafe { v8__StackFrame__GetColumn(self) as usize }
  }

  /// Returns the id of the script for the function for this StackFrame.
  /// This method will return Message::kNoScriptIdInfo if it is unable to
  /// retrieve the script id, or if kScriptId was not passed as an option when
  /// capturing the StackTrace.
  #[inline(always)]
  pub fn get_script_id(&self) -> usize {
    unsafe { v8__StackFrame__GetScriptId(self) as usize }
  }

  /// Returns the name of the resource that contains the script for the
  /// function for this StackFrame.
  #[inline(always)]
  pub fn get_script_name<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe { scope.cast_local(|_| v8__StackFrame__GetScriptName(self)) }
  }

  /// Returns the name of the resource that contains the script for the
  /// function for this StackFrame or sourceURL value if the script name
  /// is undefined and its source ends with //# sourceURL=... string or
  /// deprecated //@ sourceURL=... string.
  #[inline(always)]
  pub fn get_script_name_or_source_url<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|_| v8__StackFrame__GetScriptNameOrSourceURL(self))
    }
  }

  /// Returns the name of the function associated with this stack frame.
  #[inline(always)]
  pub fn get_function_name<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe { scope.cast_local(|_| v8__StackFrame__GetFunctionName(self)) }
  }

  /// Returns whether or not the associated function is compiled via a call to
  /// eval().
  #[inline(always)]
  pub fn is_eval(&self) -> bool {
    unsafe { v8__StackFrame__IsEval(self) }
  }

  /// Returns whether or not the associated function is called as a
  /// constructor via "new".
  #[inline(always)]
  pub fn is_constructor(&self) -> bool {
    unsafe { v8__StackFrame__IsConstructor(self) }
  }

  /// Returns whether or not the associated functions is defined in wasm.
  #[inline(always)]
  pub fn is_wasm(&self) -> bool {
    unsafe { v8__StackFrame__IsWasm(self) }
  }

  /// Returns whether or not the associated function is defined by the user.
  #[inline(always)]
  pub fn is_user_javascript(&self) -> bool {
    unsafe { v8__StackFrame__IsUserJavaScript(self) }
  }
}

impl Message {
  #[inline(always)]
  pub fn get<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__Message__Get(self)) }.unwrap()
  }

  /// Exception stack trace. By default stack traces are not captured for
  /// uncaught exceptions. SetCaptureStackTraceForUncaughtExceptions allows
  /// to change this option.
  #[inline(always)]
  pub fn get_stack_trace<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, StackTrace>> {
    unsafe { scope.cast_local(|_| v8__Message__GetStackTrace(self)) }
  }

  #[inline(always)]
  pub fn get_source_line<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Message__GetSourceLine(self, sd.get_current_context())
      })
    }
  }

  /// Returns the resource name for the script from where the function causing
  /// the error originates.
  #[inline(always)]
  pub fn get_script_resource_name<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Value>> {
    unsafe { scope.cast_local(|_| v8__Message__GetScriptResourceName(self)) }
  }

  /// Returns the number, 1-based, of the line where the error occurred.
  #[inline(always)]
  pub fn get_line_number(&self, scope: &mut HandleScope) -> Option<usize> {
    let i = unsafe {
      v8__Message__GetLineNumber(self, &*scope.get_current_context())
    };
    if i < 0 { None } else { Some(i as usize) }
  }

  /// Returns the index within the script of the first character where
  /// the error occurred.
  #[inline(always)]
  pub fn get_start_position(&self) -> int {
    unsafe { v8__Message__GetStartPosition(self) }
  }

  /// Returns the index within the script of the last character where
  /// the error occurred.
  #[inline(always)]
  pub fn get_end_position(&self) -> int {
    unsafe { v8__Message__GetEndPosition(self) }
  }

  /// Returns the Wasm function index where the error occurred. Returns -1 if
  /// message is not from a Wasm script.
  #[inline(always)]
  pub fn get_wasm_function_index(&self) -> int {
    unsafe { v8__Message__GetWasmFunctionIndex(self) }
  }

  /// Returns the error level of the message.
  #[inline(always)]
  pub fn error_level(&self) -> int {
    unsafe { v8__Message__ErrorLevel(self) }
  }

  /// Returns the index within the line of the first character where
  /// the error occurred.
  #[inline(always)]
  pub fn get_start_column(&self) -> usize {
    unsafe { v8__Message__GetStartColumn(self) as usize }
  }

  /// Returns the index within the line of the last character where
  /// the error occurred.
  #[inline(always)]
  pub fn get_end_column(&self) -> usize {
    unsafe { v8__Message__GetEndColumn(self) as usize }
  }

  /// Passes on the value set by the embedder when it fed the script from which
  /// this Message was generated to V8.
  #[inline(always)]
  pub fn is_shared_cross_origin(&self) -> bool {
    unsafe { v8__Message__IsSharedCrossOrigin(self) }
  }

  #[inline(always)]
  pub fn is_opaque(&self) -> bool {
    unsafe { v8__Message__IsOpaque(self) }
  }
}

/// Create new error objects by calling the corresponding error object
/// constructor with the message.
#[derive(Debug)]
pub struct Exception;

impl Exception {
  #[inline(always)]
  pub fn error<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
  ) -> Local<'s, Value> {
    Self::new_error_with(scope, message, v8__Exception__Error)
  }

  #[inline(always)]
  pub fn range_error<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
  ) -> Local<'s, Value> {
    Self::new_error_with(scope, message, v8__Exception__RangeError)
  }

  #[inline(always)]
  pub fn reference_error<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
  ) -> Local<'s, Value> {
    Self::new_error_with(scope, message, v8__Exception__ReferenceError)
  }

  #[inline(always)]
  pub fn syntax_error<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
  ) -> Local<'s, Value> {
    Self::new_error_with(scope, message, v8__Exception__SyntaxError)
  }

  #[inline(always)]
  pub fn type_error<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
  ) -> Local<'s, Value> {
    Self::new_error_with(scope, message, v8__Exception__TypeError)
  }

  /// Internal helper to make the above error constructors less repetitive.
  #[inline(always)]
  fn new_error_with<'s>(
    scope: &mut HandleScope<'s>,
    message: Local<String>,
    contructor: unsafe extern "C" fn(*const String) -> *const Value,
  ) -> Local<'s, Value> {
    unsafe {
      scope.enter();
      let error = scope.cast_local(|_| (contructor)(&*message)).unwrap();
      scope.exit();
      error
    }
  }

  /// Creates an error message for the given exception.
  /// Will try to reconstruct the original stack trace from the exception value,
  /// or capture the current stack trace if not available.
  #[inline(always)]
  pub fn create_message<'s>(
    scope: &mut HandleScope<'s>,
    exception: Local<Value>,
  ) -> Local<'s, Message> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Exception__CreateMessage(sd.get_isolate_ptr(), &*exception)
      })
    }
    .unwrap()
  }

  /// Returns the original stack trace that was captured at the creation time
  /// of a given exception, or an empty handle if not available.
  #[inline(always)]
  pub fn get_stack_trace<'s>(
    scope: &mut HandleScope<'s>,
    exception: Local<Value>,
  ) -> Option<Local<'s, StackTrace>> {
    unsafe { scope.cast_local(|_| v8__Exception__GetStackTrace(&*exception)) }
  }
}
