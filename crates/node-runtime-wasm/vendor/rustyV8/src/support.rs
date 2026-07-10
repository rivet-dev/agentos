use std::any::Any;
use std::any::type_name;
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::convert::AsMut;
use std::convert::AsRef;
use std::convert::TryFrom;
use std::convert::identity;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::mem::align_of;
use std::mem::forget;
use std::mem::needs_drop;
use std::mem::size_of;
use std::mem::take;
use std::mem::transmute_copy;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr::NonNull;
use std::ptr::drop_in_place;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::yield_now;
use std::time::Duration;
use std::time::Instant;

// TODO use libc::intptr_t when stable.
// https://doc.rust-lang.org/1.7.0/libc/type.intptr_t.html
#[allow(non_camel_case_types)]
pub type intptr_t = isize;

// TODO use libc::size_t when stable.
// https://doc.rust-lang.org/1.7.0/libc/type.size_t.html
#[allow(non_camel_case_types)]
pub type size_t = usize;

pub use std::os::raw::c_char as char;
pub use std::os::raw::c_int as int;
pub use std::os::raw::c_long as long;

pub type Opaque = [u8; 0];

/// Pointer to object allocated on the C++ heap. The pointer may be null.
#[repr(transparent)]
#[derive(Debug)]
pub struct UniquePtr<T: ?Sized>(Option<UniqueRef<T>>);

impl<T: ?Sized> UniquePtr<T> {
  pub fn is_null(&self) -> bool {
    self.0.is_none()
  }

  pub fn as_ref(&self) -> Option<&UniqueRef<T>> {
    self.0.as_ref()
  }

  pub fn as_mut(&mut self) -> Option<&mut UniqueRef<T>> {
    self.0.as_mut()
  }

  pub fn take(&mut self) -> Option<UniqueRef<T>> {
    take(&mut self.0)
  }

  pub fn unwrap(self) -> UniqueRef<T> {
    self.0.unwrap()
  }
}

impl<T> UniquePtr<T> {
  pub unsafe fn from_raw(ptr: *mut T) -> Self {
    assert_unique_ptr_layout_compatible::<Self, T>();
    Self(unsafe { UniqueRef::try_from_raw(ptr) })
  }

  pub fn into_raw(self) -> *mut T {
    self
      .0
      .map_or_else(null_mut, |unique_ref| unique_ref.into_raw())
  }
}

impl<T: Shared> UniquePtr<T> {
  pub fn make_shared(self) -> SharedPtr<T> {
    self.into()
  }
}

impl<T> Default for UniquePtr<T> {
  fn default() -> Self {
    assert_unique_ptr_layout_compatible::<Self, T>();
    Self(None)
  }
}

impl<T> From<UniqueRef<T>> for UniquePtr<T> {
  fn from(unique_ref: UniqueRef<T>) -> Self {
    assert_unique_ptr_layout_compatible::<Self, T>();
    Self(Some(unique_ref))
  }
}

/// Pointer to object allocated on the C++ heap. The pointer may not be null.
#[repr(transparent)]
#[derive(Debug)]
pub struct UniqueRef<T: ?Sized>(NonNull<T>);

impl<T> UniqueRef<T> {
  pub(crate) unsafe fn try_from_raw(ptr: *mut T) -> Option<Self> {
    assert_unique_ptr_layout_compatible::<Self, T>();
    NonNull::new(ptr).map(Self)
  }

  pub(crate) unsafe fn from_raw(ptr: *mut T) -> Self {
    assert_unique_ptr_layout_compatible::<Self, T>();
    unsafe { Self::try_from_raw(ptr).unwrap() }
  }

  pub fn into_raw(self) -> *mut T {
    let ptr = self.0.as_ptr();
    forget(self);
    ptr
  }
}

impl<T: Shared> UniqueRef<T> {
  pub fn make_shared(self) -> SharedRef<T> {
    self.into()
  }
}

impl<T: ?Sized> Drop for UniqueRef<T> {
  fn drop(&mut self) {
    unsafe { drop_in_place(self.0.as_ptr()) }
  }
}

impl<T: ?Sized> Deref for UniqueRef<T> {
  type Target = T;
  fn deref(&self) -> &Self::Target {
    unsafe { self.0.as_ref() }
  }
}

impl<T: ?Sized> DerefMut for UniqueRef<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { self.0.as_mut() }
  }
}

impl<T: ?Sized> AsRef<T> for UniqueRef<T> {
  fn as_ref(&self) -> &T {
    self
  }
}

impl<T: ?Sized> AsMut<T> for UniqueRef<T> {
  fn as_mut(&mut self) -> &mut T {
    self
  }
}

impl<T: ?Sized> Borrow<T> for UniqueRef<T> {
  fn borrow(&self) -> &T {
    self
  }
}

impl<T: ?Sized> BorrowMut<T> for UniqueRef<T> {
  fn borrow_mut(&mut self) -> &mut T {
    self
  }
}

fn assert_unique_ptr_layout_compatible<U, T>() {
  // Assert that `U` (a `UniqueRef` or `UniquePtr`) has the same memory layout
  // as a raw C pointer.
  assert_eq!(size_of::<U>(), size_of::<*mut T>());
  assert_eq!(align_of::<U>(), align_of::<*mut T>());

  // Assert that `T` (probably) implements `Drop`. If it doesn't, a regular
  // reference should be used instead of UniquePtr/UniqueRef.
  assert!(needs_drop::<T>());
}

pub trait Shared
where
  Self: Sized,
{
  fn clone(shared_ptr: &SharedPtrBase<Self>) -> SharedPtrBase<Self>;
  fn from_unique_ptr(unique_ptr: UniquePtr<Self>) -> SharedPtrBase<Self>;
  fn get(shared_ptr: &SharedPtrBase<Self>) -> *const Self;
  fn reset(shared_ptr: &mut SharedPtrBase<Self>);
  fn use_count(shared_ptr: &SharedPtrBase<Self>) -> long;
}

/// Private base type which is shared by the `SharedPtr` and `SharedRef`
/// implementations.
#[repr(C)]
#[derive(Eq, Debug, PartialEq)]
pub struct SharedPtrBase<T: Shared>([usize; 2], PhantomData<T>);

unsafe impl<T: Shared + Sync> Send for SharedPtrBase<T> {}
unsafe impl<T: Shared + Sync> Sync for SharedPtrBase<T> {}

impl<T: Shared> Default for SharedPtrBase<T> {
  fn default() -> Self {
    Self([0usize; 2], PhantomData)
  }
}

impl<T: Shared> Drop for SharedPtrBase<T> {
  fn drop(&mut self) {
    <T as Shared>::reset(self);
  }
}

/// Wrapper around a C++ shared_ptr. A shared_ptr may be be null.
#[repr(C)]
#[derive(Debug)]
pub struct SharedPtr<T: Shared>(SharedPtrBase<T>);

impl<T: Shared> SharedPtr<T> {
  /// Asserts that the number of references to the shared inner value is equal
  /// to the `expected` count.
  ///
  /// This function relies on the C++ method `std::shared_ptr::use_count()`,
  /// which usually performs a relaxed load. This function will repeatedly call
  /// `use_count()` until it returns the expected value, for up to one second.
  /// Therefore it should probably not be used in performance critical code.
  #[track_caller]
  pub fn assert_use_count_eq(&self, expected: usize) {
    assert_shared_ptr_use_count_eq("SharedPtr", &self.0, expected);
  }

  pub fn is_null(&self) -> bool {
    <T as Shared>::get(&self.0).is_null()
  }

  pub fn take(&mut self) -> Option<SharedRef<T>> {
    if self.is_null() {
      None
    } else {
      let base = take(&mut self.0);
      Some(SharedRef(base))
    }
  }

  pub fn unwrap(self) -> SharedRef<T> {
    assert!(!self.is_null());
    SharedRef(self.0)
  }
}

impl<T: Shared> Clone for SharedPtr<T> {
  fn clone(&self) -> Self {
    Self(<T as Shared>::clone(&self.0))
  }
}

impl<T: Shared> Default for SharedPtr<T> {
  fn default() -> Self {
    Self(Default::default())
  }
}

impl<T, U> From<U> for SharedPtr<T>
where
  T: Shared,
  U: Into<UniquePtr<T>>,
{
  fn from(unique_ptr: U) -> Self {
    let unique_ptr = unique_ptr.into();
    Self(<T as Shared>::from_unique_ptr(unique_ptr))
  }
}

impl<T: Shared> From<SharedRef<T>> for SharedPtr<T> {
  fn from(mut shared_ref: SharedRef<T>) -> Self {
    Self(take(&mut shared_ref.0))
  }
}

/// Wrapper around a C++ shared_ptr. The shared_ptr is assumed to contain a
/// value and may not be null.
#[repr(C)]
#[derive(Debug)]
pub struct SharedRef<T: Shared>(SharedPtrBase<T>);

impl<T: Shared> SharedRef<T> {
  /// Asserts that the number of references to the shared inner value is equal
  /// to the `expected` count.
  ///
  /// This function relies on the C++ method `std::shared_ptr::use_count()`,
  /// which usually performs a relaxed load. This function will repeatedly call
  /// `use_count()` until it returns the expected value, for up to one second.
  /// Therefore it should probably not be used in performance critical code.
  #[track_caller]
  pub fn assert_use_count_eq(&self, expected: usize) {
    assert_shared_ptr_use_count_eq("SharedRef", &self.0, expected);
  }
}

impl<T: Shared> Clone for SharedRef<T> {
  fn clone(&self) -> Self {
    Self(<T as Shared>::clone(&self.0))
  }
}

impl<T: Shared> From<UniqueRef<T>> for SharedRef<T> {
  fn from(unique_ref: UniqueRef<T>) -> Self {
    SharedPtr::from(unique_ref).unwrap()
  }
}

impl<T: Shared> Deref for SharedRef<T> {
  type Target = T;
  fn deref(&self) -> &Self::Target {
    unsafe { &*(<T as Shared>::get(&self.0)) }
  }
}

impl<T: Shared> AsRef<T> for SharedRef<T> {
  fn as_ref(&self) -> &T {
    self
  }
}

impl<T: Shared> Borrow<T> for SharedRef<T> {
  fn borrow(&self) -> &T {
    self
  }
}

#[track_caller]
fn assert_shared_ptr_use_count_eq<T: Shared>(
  wrapper_type_name: &str,
  shared_ptr: &SharedPtrBase<T>,
  expected: usize,
) {
  let mut actual = T::use_count(shared_ptr);
  let ok = match long::try_from(expected) {
    Err(_) => false, // Non-`long` value can never match actual use count.
    Ok(expected) if actual == expected => true, // Fast path.
    Ok(expected) => {
      pub const RETRY_TIMEOUT: Duration = Duration::from_secs(1);
      let start = Instant::now();
      loop {
        yield_now();
        actual = T::use_count(shared_ptr);
        if actual == expected {
          break true;
        } else if start.elapsed() > RETRY_TIMEOUT {
          break false;
        }
      }
    }
  };
  assert!(
    ok,
    "assertion failed: `{wrapper_type_name}<{}>` reference count does not match expectation\
       \n   actual: {actual}\
       \n expected: {expected}",
    type_name::<T>(),
  );
}

/// A trait for values with static lifetimes that are allocated at a fixed
/// address in memory. Practically speaking, that means they're either a
/// `&'static` reference, or they're heap-allocated in a `Arc`, `Box`, `Rc`,
/// `UniqueRef`, `SharedRef` or `Vec`.
pub trait Allocated<T: ?Sized>:
  Deref<Target = T> + Borrow<T> + 'static
{
}
impl<A, T: ?Sized> Allocated<T> for A where
  A: Deref<Target = T> + Borrow<T> + 'static
{
}

pub(crate) enum Allocation<T: ?Sized + 'static> {
  Static(&'static T),
  Arc(Arc<T>),
  Box(Box<T>),
  Rc(Rc<T>),
  UniqueRef(UniqueRef<T>),
  Other(Box<dyn Borrow<T> + 'static>),
  // Note: it would be nice to add `SharedRef` to this list, but it
  // requires the `T: Shared` bound, and it's unfortunately not possible
  // to set bounds on individual enum variants.
}

impl<T: ?Sized + 'static> Allocation<T> {
  unsafe fn transmute_wrap<Abstract, Concrete>(
    value: Abstract,
    wrap: fn(Concrete) -> Self,
  ) -> Self {
    assert_eq!(size_of::<Abstract>(), size_of::<Concrete>());
    let wrapped = wrap(unsafe { transmute_copy(&value) });
    forget(value);
    wrapped
  }

  fn try_wrap<Abstract: 'static, Concrete: 'static>(
    value: Abstract,
    wrap: fn(Concrete) -> Self,
  ) -> Result<Self, Abstract> {
    if <dyn Any>::is::<Concrete>(&value) {
      Ok(unsafe { Self::transmute_wrap(value, wrap) })
    } else {
      Err(value)
    }
  }

  pub fn of<Abstract: Deref<Target = T> + Borrow<T> + 'static>(
    a: Abstract,
  ) -> Self {
    Self::try_wrap(a, identity)
      .or_else(|a| Self::try_wrap(a, Self::Static))
      .or_else(|a| Self::try_wrap(a, Self::Arc))
      .or_else(|a| Self::try_wrap(a, Self::Box))
      .or_else(|a| Self::try_wrap(a, Self::Rc))
      .or_else(|a| Self::try_wrap(a, Self::UniqueRef))
      .unwrap_or_else(|a| Self::Other(Box::from(a)))
  }
}

impl<T: Debug + ?Sized> Debug for Allocation<T> {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    match self {
      Allocation::Arc(r) => f.debug_tuple("Arc").field(&r).finish(),
      Allocation::Box(b) => f.debug_tuple("Box").field(&b).finish(),
      Allocation::Other(_) => f.debug_tuple("Other").finish(),
      Allocation::Rc(r) => f.debug_tuple("Rc").field(&r).finish(),
      Allocation::Static(s) => f.debug_tuple("Static").field(&s).finish(),
      Allocation::UniqueRef(u) => f.debug_tuple("UniqueRef").field(&u).finish(),
    }
  }
}

impl<T: ?Sized> Deref for Allocation<T> {
  type Target = T;
  fn deref(&self) -> &Self::Target {
    match self {
      Self::Static(v) => v.borrow(),
      Self::Arc(v) => v.borrow(),
      Self::Box(v) => v.borrow(),
      Self::Rc(v) => v.borrow(),
      Self::UniqueRef(v) => v.borrow(),
      Self::Other(v) => (**v).borrow(),
    }
  }
}

impl<T: ?Sized> AsRef<T> for Allocation<T> {
  fn as_ref(&self) -> &T {
    self
  }
}

impl<T: ?Sized> Borrow<T> for Allocation<T> {
  fn borrow(&self) -> &T {
    self
  }
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum MaybeBool {
  JustFalse = 0,
  JustTrue = 1,
  Nothing = 2,
}

impl From<MaybeBool> for Option<bool> {
  fn from(b: MaybeBool) -> Self {
    match b {
      MaybeBool::JustFalse => Some(false),
      MaybeBool::JustTrue => Some(true),
      MaybeBool::Nothing => None,
    }
  }
}

impl From<Option<bool>> for MaybeBool {
  fn from(option: Option<bool>) -> Self {
    match option {
      Some(false) => MaybeBool::JustFalse,
      Some(true) => MaybeBool::JustTrue,
      None => MaybeBool::Nothing,
    }
  }
}

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct CxxVTable(pub *const Opaque);

#[allow(unused)]
#[derive(Copy, Clone, Debug)]
pub struct RustVTable<DynT>(pub *const Opaque, pub PhantomData<DynT>);

#[derive(Debug)]
pub struct FieldOffset<F>(usize, PhantomData<F>);

unsafe impl<F> Send for FieldOffset<F> where F: Send {}
unsafe impl<F> Sync for FieldOffset<F> where F: Sync {}

impl<F> Copy for FieldOffset<F> {}

impl<F> Clone for FieldOffset<F> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<F> FieldOffset<F> {
  pub fn from_ptrs<E>(embedder_ptr: *const E, field_ptr: *const F) -> Self {
    let embedder_addr = embedder_ptr as usize;
    let field_addr = field_ptr as usize;
    assert!(field_addr >= embedder_addr);
    assert!((field_addr + size_of::<F>()) <= (embedder_addr + size_of::<E>()));
    Self(field_addr - embedder_addr, PhantomData)
  }

  #[allow(clippy::wrong_self_convention)]
  pub unsafe fn to_embedder<E>(self, field: &F) -> &E {
    unsafe {
      (((field as *const _ as usize) - self.0) as *const E)
        .as_ref()
        .unwrap()
    }
  }

  #[allow(clippy::wrong_self_convention)]
  pub unsafe fn to_embedder_mut<E>(self, field: &mut F) -> &mut E {
    unsafe {
      (((field as *mut _ as usize) - self.0) as *mut E)
        .as_mut()
        .unwrap()
    }
  }
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct Maybe<T> {
  has_value: bool,
  value: T,
}

impl<T> From<Maybe<T>> for Option<T> {
  fn from(maybe: Maybe<T>) -> Self {
    if maybe.has_value {
      Some(maybe.value)
    } else {
      None
    }
  }
}

pub trait UnitType
where
  Self: Copy + Sized,
{
  #[inline(always)]
  fn get() -> Self {
    UnitValue::<Self>::get()
  }
}

impl<T> UnitType for T where T: Copy + Sized {}

#[derive(Copy, Clone, Debug)]
struct UnitValue<T>(PhantomData<T>)
where
  Self: Sized;

impl<T> UnitValue<T>
where
  Self: Copy + Sized,
{
  const SELF: Self = Self::new_checked();

  const fn new_checked() -> Self {
    // Statically assert that T is indeed a unit type.
    let size_must_be_0 = size_of::<T>();
    let s = Self(PhantomData::<T>);
    [s][size_must_be_0]
  }

  #[inline(always)]
  fn get_checked(self) -> T {
    // This run-time check serves just as a backup for the compile-time
    // check when Self::SELF is initialized.
    assert_eq!(size_of::<T>(), 0);
    unsafe { std::mem::MaybeUninit::<T>::zeroed().assume_init() }
  }

  #[inline(always)]
  pub fn get() -> T {
    // Accessing the Self::SELF is necessary to make the compile-time type check
    // work.
    Self::SELF.get_checked()
  }
}

#[derive(Debug)]
pub struct DefaultTag;

#[derive(Debug)]
pub struct IdenticalConversionTag;

pub trait MapFnFrom<F, Tag = DefaultTag>
where
  F: UnitType,
  Self: Sized,
{
  fn mapping() -> Self;

  #[inline(always)]
  fn map_fn_from(_: F) -> Self {
    Self::mapping()
  }
}

impl<F> MapFnFrom<F, IdenticalConversionTag> for F
where
  Self: UnitType,
{
  #[inline(always)]
  fn mapping() -> Self {
    Self::get()
  }
}

pub trait MapFnTo<T, Tag = DefaultTag>
where
  Self: UnitType,
  T: Sized,
{
  fn mapping() -> T;

  #[inline(always)]
  fn map_fn_to(self) -> T {
    Self::mapping()
  }
}

impl<F, T, Tag> MapFnTo<T, Tag> for F
where
  Self: UnitType,
  T: MapFnFrom<F, Tag>,
{
  #[inline(always)]
  fn mapping() -> T {
    T::map_fn_from(F::get())
  }
}

pub trait CFnFrom<F>
where
  Self: Sized,
  F: UnitType,
{
  fn mapping() -> Self;

  #[inline(always)]
  fn c_fn_from(_: F) -> Self {
    Self::mapping()
  }
}

macro_rules! impl_c_fn_from {
  ($($arg:ident: $ty:ident),*) => {
    impl<F, R, $($ty),*> CFnFrom<F> for unsafe extern "C" fn($($ty),*) -> R
    where
      F: UnitType + Fn($($ty),*) -> R,
    {
      #[inline(always)]
      fn mapping() -> Self {
        unsafe extern "C" fn c_fn<F, R, $($ty),*>($($arg: $ty),*) -> R
        where
          F: UnitType + Fn($($ty),*) -> R,
        {
          (F::get())($($arg),*)
        }
        c_fn::<F, R, $($ty),*>
      }
    }
  };
}

impl_c_fn_from!();
impl_c_fn_from!(a0: A0);
impl_c_fn_from!(a0: A0, a1: A1);
impl_c_fn_from!(a0: A0, a1: A1, a2: A2);
impl_c_fn_from!(a0: A0, a1: A1, a2: A2, a3: A3);
impl_c_fn_from!(a0: A0, a1: A1, a2: A2, a3: A3, a4: A4);
impl_c_fn_from!(a0: A0, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5);
impl_c_fn_from!(a0: A0, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6);

pub trait ToCFn<T>
where
  Self: UnitType,
  T: Sized,
{
  fn mapping() -> T;

  #[inline(always)]
  fn to_c_fn(self) -> T {
    Self::mapping()
  }
}

impl<F, T> ToCFn<T> for F
where
  Self: UnitType,
  T: CFnFrom<F>,
{
  #[inline(always)]
  fn mapping() -> T {
    T::c_fn_from(F::get())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::ptr::null;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::Ordering;

  #[derive(Eq, PartialEq)]
  struct MockSharedObj {
    pub inner: u32,
  }

  impl MockSharedObj {
    const INSTANCE_A: Self = Self { inner: 11111 };
    const INSTANCE_B: Self = Self { inner: 22222 };

    const SHARED_PTR_BASE_A: SharedPtrBase<Self> =
      SharedPtrBase([1, 1], PhantomData);
    const SHARED_PTR_BASE_B: SharedPtrBase<Self> =
      SharedPtrBase([2, 2], PhantomData);
  }

  impl Shared for MockSharedObj {
    fn clone(_: &SharedPtrBase<Self>) -> SharedPtrBase<Self> {
      unimplemented!()
    }

    fn from_unique_ptr(_: UniquePtr<Self>) -> SharedPtrBase<Self> {
      unimplemented!()
    }

    fn get(p: &SharedPtrBase<Self>) -> *const Self {
      match p {
        &Self::SHARED_PTR_BASE_A => &Self::INSTANCE_A,
        &Self::SHARED_PTR_BASE_B => &Self::INSTANCE_B,
        p if p == &Default::default() => null(),
        _ => unreachable!(),
      }
    }

    fn reset(p: &mut SharedPtrBase<Self>) {
      forget(take(p));
    }

    fn use_count(p: &SharedPtrBase<Self>) -> long {
      match p {
        &Self::SHARED_PTR_BASE_A => 1,
        &Self::SHARED_PTR_BASE_B => 2,
        p if p == &Default::default() => 0,
        _ => unreachable!(),
      }
    }
  }

  #[test]
  fn shared_ptr_and_shared_ref() {
    let mut shared_ptr_a1 = SharedPtr(MockSharedObj::SHARED_PTR_BASE_A);
    assert!(!shared_ptr_a1.is_null());
    shared_ptr_a1.assert_use_count_eq(1);

    let shared_ref_a: SharedRef<_> = shared_ptr_a1.take().unwrap();
    assert_eq!(shared_ref_a.inner, 11111);
    shared_ref_a.assert_use_count_eq(1);

    assert!(shared_ptr_a1.is_null());
    shared_ptr_a1.assert_use_count_eq(0);

    let shared_ptr_a2: SharedPtr<_> = shared_ref_a.into();
    assert!(!shared_ptr_a2.is_null());
    shared_ptr_a2.assert_use_count_eq(1);
    assert_eq!(shared_ptr_a2.unwrap().inner, 11111);

    let mut shared_ptr_b1 = SharedPtr(MockSharedObj::SHARED_PTR_BASE_B);
    assert!(!shared_ptr_b1.is_null());
    shared_ptr_b1.assert_use_count_eq(2);

    let shared_ref_b: SharedRef<_> = shared_ptr_b1.take().unwrap();
    assert_eq!(shared_ref_b.inner, 22222);
    shared_ref_b.assert_use_count_eq(2);

    assert!(shared_ptr_b1.is_null());
    shared_ptr_b1.assert_use_count_eq(0);

    let shared_ptr_b2: SharedPtr<_> = shared_ref_b.into();
    assert!(!shared_ptr_b2.is_null());
    shared_ptr_b2.assert_use_count_eq(2);
    assert_eq!(shared_ptr_b2.unwrap().inner, 22222);
  }

  #[test]
  #[should_panic(expected = "assertion failed: \
      `SharedPtr<v8::support::tests::MockSharedObj>` reference count \
      does not match expectation")]
  fn shared_ptr_use_count_assertion_failed() {
    let shared_ptr: SharedPtr<MockSharedObj> = Default::default();
    shared_ptr.assert_use_count_eq(3);
  }

  #[test]
  #[should_panic(expected = "assertion failed: \
      `SharedRef<v8::support::tests::MockSharedObj>` reference count \
      does not match expectation")]
  fn shared_ref_use_count_assertion_failed() {
    let shared_ref = SharedRef(MockSharedObj::SHARED_PTR_BASE_B);
    shared_ref.assert_use_count_eq(7);
  }

  static TEST_OBJ_DROPPED: AtomicBool = AtomicBool::new(false);

  struct TestObj {
    pub id: u32,
  }

  impl Drop for TestObj {
    fn drop(&mut self) {
      assert!(!TEST_OBJ_DROPPED.swap(true, Ordering::SeqCst));
    }
  }

  struct TestObjRef(TestObj);

  impl Deref for TestObjRef {
    type Target = TestObj;

    fn deref(&self) -> &TestObj {
      &self.0
    }
  }

  impl Borrow<TestObj> for TestObjRef {
    fn borrow(&self) -> &TestObj {
      self
    }
  }

  #[test]
  fn allocation() {
    // Static.
    static STATIC_OBJ: TestObj = TestObj { id: 1 };
    let owner = Allocation::of(&STATIC_OBJ);
    match owner {
      Allocation::Static(_) => assert_eq!(owner.id, 1),
      _ => panic!(),
    }
    drop(owner);
    assert!(!TEST_OBJ_DROPPED.load(Ordering::SeqCst));

    // Arc.
    let owner = Allocation::of(Arc::new(TestObj { id: 2 }));
    match owner {
      Allocation::Arc(_) => assert_eq!(owner.id, 2),
      _ => panic!(),
    }
    drop(owner);
    assert!(TEST_OBJ_DROPPED.swap(false, Ordering::SeqCst));

    // Box.
    let owner = Allocation::of(Box::new(TestObj { id: 3 }));
    match owner {
      Allocation::Box(_) => assert_eq!(owner.id, 3),
      _ => panic!(),
    }
    drop(owner);
    assert!(TEST_OBJ_DROPPED.swap(false, Ordering::SeqCst));

    // Rc.
    let owner = Allocation::of(Rc::new(TestObj { id: 4 }));
    match owner {
      Allocation::Rc(_) => assert_eq!(owner.id, 4),
      _ => panic!(),
    }
    drop(owner);
    assert!(TEST_OBJ_DROPPED.swap(false, Ordering::SeqCst));

    // Other.
    let owner = Allocation::of(TestObjRef(TestObj { id: 5 }));
    match owner {
      Allocation::Other(_) => assert_eq!(owner.id, 5),
      _ => panic!(),
    }
    drop(owner);
    assert!(TEST_OBJ_DROPPED.swap(false, Ordering::SeqCst));

    // Contents of Vec should not be moved.
    let vec = vec![1u8, 2, 3, 5, 8, 13, 21];
    let vec_element_ptrs =
      vec.iter().map(|i| i as *const u8).collect::<Vec<_>>();
    let owner = Allocation::of(vec);
    match owner {
      Allocation::Other(_) => {}
      _ => panic!(),
    }
    owner
      .iter()
      .map(|i| i as *const u8)
      .zip(vec_element_ptrs)
      .for_each(|(p1, p2)| assert_eq!(p1, p2));
  }
}
