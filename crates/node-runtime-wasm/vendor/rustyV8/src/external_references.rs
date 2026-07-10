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
use crate::support::intptr_t;
use std::ffi::c_void;
use std::fmt::Debug;

#[derive(Clone, Copy)]
pub union ExternalReference<'s> {
  pub function: FunctionCallback,
  pub named_getter: NamedGetterCallback<'s>,
  pub named_setter: NamedSetterCallback<'s>,
  pub named_definer: NamedDefinerCallback<'s>,
  pub named_deleter: NamedDeleterCallback<'s>,
  pub named_query: NamedQueryCallback<'s>,
  pub indexed_getter: IndexedGetterCallback<'s>,
  pub indexed_setter: IndexedSetterCallback<'s>,
  pub indexed_definer: IndexedDefinerCallback<'s>,
  pub indexed_deleter: IndexedDeleterCallback<'s>,
  pub indexed_query: IndexedQueryCallback<'s>,
  pub enumerator: PropertyEnumeratorCallback<'s>,
  pub message: MessageCallback,
  pub pointer: *mut c_void,
  pub type_info: *const CFunctionInfo,
}

impl Debug for ExternalReference<'_> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    // SAFETY: All union fields are the same size
    unsafe { (self.pointer).fmt(f) }
  }
}

#[derive(Debug, Clone)]
pub struct ExternalReferences {
  null_terminated: Vec<intptr_t>,
}

unsafe impl Sync for ExternalReferences {}

impl ExternalReferences {
  #[inline(always)]
  pub fn new(refs: &[ExternalReference]) -> Self {
    let null_terminated = refs
      .iter()
      .map(|&r| unsafe { std::mem::transmute(r) })
      .chain(std::iter::once(0)) // Add null terminator.
      .collect::<Vec<intptr_t>>();
    Self { null_terminated }
  }

  #[inline(always)]
  pub fn as_ptr(&self) -> *const intptr_t {
    self.null_terminated.as_ptr()
  }
}

impl std::ops::Deref for ExternalReferences {
  type Target = [intptr_t];
  fn deref(&self) -> &Self::Target {
    &self.null_terminated
  }
}

impl std::borrow::Borrow<[intptr_t]> for ExternalReferences {
  fn borrow(&self) -> &[intptr_t] {
    self
  }
}
