#[repr(C)]
#[derive(Debug, Eq, PartialEq)]
pub struct PropertyAttribute(u32);

impl PropertyAttribute {
  /// No property attributes.
  pub const NONE: Self = Self(0);

  /// Not writable. Corresponds to
  /// `Object.defineProperty(o, "p", { writable: false })`.
  pub const READ_ONLY: Self = Self(1 << 0);

  /// Not enumerable. Corresponds to
  /// `Object.defineProperty(o, "p", { enumerable: false })`.
  pub const DONT_ENUM: Self = Self(1 << 1);

  /// Not configurable. Corresponds to
  /// `Object.defineProperty(o, "p", { configurable: false })`.
  pub const DONT_DELETE: Self = Self(1 << 2);

  /// Test if no property attributes are set.
  #[inline(always)]
  pub fn is_none(&self) -> bool {
    *self == PropertyAttribute::NONE
  }

  /// Test if the read-only property attribute is set.
  #[inline(always)]
  pub fn is_read_only(&self) -> bool {
    self.has(Self::READ_ONLY)
  }

  /// Test if the non-enumerable property attribute is set.
  #[inline(always)]
  pub fn is_dont_enum(&self) -> bool {
    self.has(Self::DONT_ENUM)
  }

  /// Test if the non-configurable property attribute is set.
  #[inline(always)]
  pub fn is_dont_delete(&self) -> bool {
    self.has(Self::DONT_DELETE)
  }

  #[inline(always)]
  fn has(&self, that: Self) -> bool {
    let Self(lhs) = self;
    let Self(rhs) = that;
    0 != lhs & rhs
  }

  pub fn as_u32(&self) -> u32 {
    let Self(bits) = self;
    *bits
  }
}

// Identical to #[derive(Default)] but arguably clearer when made explicit.
impl Default for PropertyAttribute {
  fn default() -> Self {
    Self::NONE
  }
}

impl std::ops::BitOr for PropertyAttribute {
  type Output = Self;

  fn bitor(self, Self(rhs): Self) -> Self {
    let Self(lhs) = self;
    Self(lhs | rhs)
  }
}

#[test]
fn test_attr() {
  assert!(PropertyAttribute::NONE.is_none());
  assert!(!PropertyAttribute::NONE.is_read_only());
  assert!(!PropertyAttribute::NONE.is_dont_enum());
  assert!(!PropertyAttribute::NONE.is_dont_delete());

  assert!(!PropertyAttribute::READ_ONLY.is_none());
  assert!(PropertyAttribute::READ_ONLY.is_read_only());
  assert!(!PropertyAttribute::READ_ONLY.is_dont_enum());
  assert!(!PropertyAttribute::READ_ONLY.is_dont_delete());

  assert!(!PropertyAttribute::DONT_ENUM.is_none());
  assert!(!PropertyAttribute::DONT_ENUM.is_read_only());
  assert!(PropertyAttribute::DONT_ENUM.is_dont_enum());
  assert!(!PropertyAttribute::DONT_ENUM.is_dont_delete());

  assert!(!PropertyAttribute::DONT_DELETE.is_none());
  assert!(!PropertyAttribute::DONT_DELETE.is_read_only());
  assert!(!PropertyAttribute::DONT_DELETE.is_dont_enum());
  assert!(PropertyAttribute::DONT_DELETE.is_dont_delete());

  assert_eq!(PropertyAttribute::NONE, Default::default());
  assert_eq!(
    PropertyAttribute::READ_ONLY,
    PropertyAttribute::NONE | PropertyAttribute::READ_ONLY
  );

  let attr = PropertyAttribute::READ_ONLY | PropertyAttribute::DONT_ENUM;
  assert!(!attr.is_none());
  assert!(attr.is_read_only());
  assert!(attr.is_dont_enum());
  assert!(!attr.is_dont_delete());

  let attr = PropertyAttribute::READ_ONLY
    | PropertyAttribute::READ_ONLY
    | PropertyAttribute::DONT_ENUM;
  assert!(!attr.is_none());
  assert!(attr.is_read_only());
  assert!(attr.is_dont_enum());
  assert!(!attr.is_dont_delete());
}
