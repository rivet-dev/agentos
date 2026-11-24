// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::FunctionCallback;
use crate::IndexedDefinerCallback;
use crate::IndexedDeleterCallback;
use crate::IndexedGetterCallback;
use crate::IndexedQueryCallback;
use crate::IndexedSetterCallback;
use crate::MessageCallback;
use crate::NamedDefinerCallback;
use crate::NamedDeleterCallback;
use crate::NamedGetterCallback;
use crate::NamedQueryCallback;
use crate::NamedSetterCallback;
use crate::PropertyEnumeratorCallback;
use crate::fast_api::CFunctionInfo;
use std::ffi::c_void;
use std::fmt::Debug;

#[derive(Clone, Copy)]
pub union ExternalReference {
  pub function: FunctionCallback,
  pub named_getter: NamedGetterCallback,
  pub named_setter: NamedSetterCallback,
  pub named_definer: NamedDefinerCallback,
  pub named_deleter: NamedDeleterCallback,
  pub named_query: NamedQueryCallback,
  pub indexed_getter: IndexedGetterCallback,
  pub indexed_setter: IndexedSetterCallback,
  pub indexed_definer: IndexedDefinerCallback,
  pub indexed_deleter: IndexedDeleterCallback,
  pub indexed_query: IndexedQueryCallback,
  pub enumerator: PropertyEnumeratorCallback,
  pub message: MessageCallback,
  pub pointer: *mut c_void,
  pub type_info: *const CFunctionInfo,
}

impl Debug for ExternalReference {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    // SAFETY: All union fields are the same size
    unsafe { (self.pointer).fmt(f) }
  }
}

impl PartialEq for ExternalReference {
  fn eq(&self, other: &Self) -> bool {
    unsafe { self.pointer.eq(&other.pointer) }
  }
}
