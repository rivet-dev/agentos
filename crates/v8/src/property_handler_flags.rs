// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

#[repr(C)]
#[derive(Debug, Eq, PartialEq)]
pub struct PropertyHandlerFlags(u32);

impl PropertyHandlerFlags {
  /// None.
  pub const NONE: Self = Self(0);

  /// Will not call into interceptor for properties on the receiver or prototype
  /// chain, i.e., only call into interceptor for properties that do not exist.
  /// Currently only valid for named interceptors.
  pub const NON_MASKING: Self = Self(1 << 0);

  /// Will not call into interceptor for symbol lookup.  Only meaningful for
  /// named interceptors.
  pub const ONLY_INTERCEPT_STRINGS: Self = Self(1 << 1);

  /// The getter, query, enumerator callbacks do not produce side effects.
  pub const HAS_NO_SIDE_EFFECT: Self = Self(1 << 2);

  /// Test if no property handler flags are set.
  #[inline(always)]
  pub fn is_none(&self) -> bool {
    *self == Self::NONE
  }

  /// Test if the non-masking property handler flag is set.
  #[inline(always)]
  pub fn is_non_masking(&self) -> bool {
    self.has(Self::NON_MASKING)
  }

  /// Test if the only-intercept-strings property handler flag is set.
  #[inline(always)]
  pub fn is_only_intercept_strings(&self) -> bool {
    self.has(Self::ONLY_INTERCEPT_STRINGS)
  }

  /// Test if the has-no-side-effect property handler flag is set.
  #[inline(always)]
  pub fn is_has_no_side_effect(&self) -> bool {
    self.has(Self::HAS_NO_SIDE_EFFECT)
  }

  #[inline(always)]
  fn has(&self, that: Self) -> bool {
    let Self(lhs) = self;
    let Self(rhs) = that;
    0 != lhs & rhs
  }
}

// Identical to #[derive(Default)] but arguably clearer when made explicit.
impl Default for PropertyHandlerFlags {
  fn default() -> Self {
    Self::NONE
  }
}

impl std::ops::BitOr for PropertyHandlerFlags {
  type Output = Self;

  fn bitor(self, Self(rhs): Self) -> Self {
    let Self(lhs) = self;
    Self(lhs | rhs)
  }
}

#[test]
fn test_attr() {
  assert!(PropertyHandlerFlags::NONE.is_none());
  assert!(!PropertyHandlerFlags::NONE.is_non_masking());
  assert!(!PropertyHandlerFlags::NONE.is_only_intercept_strings());
  assert!(!PropertyHandlerFlags::NONE.is_has_no_side_effect());

  assert!(!PropertyHandlerFlags::NON_MASKING.is_none());
  assert!(PropertyHandlerFlags::NON_MASKING.is_non_masking());
  assert!(!PropertyHandlerFlags::NON_MASKING.is_only_intercept_strings());
  assert!(!PropertyHandlerFlags::NON_MASKING.is_has_no_side_effect());

  assert!(!PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS.is_none());
  assert!(!PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS.is_non_masking());
  assert!(
    PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS.is_only_intercept_strings()
  );
  assert!(
    !PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS.is_has_no_side_effect()
  );

  assert!(!PropertyHandlerFlags::HAS_NO_SIDE_EFFECT.is_none());
  assert!(!PropertyHandlerFlags::HAS_NO_SIDE_EFFECT.is_non_masking());
  assert!(
    !PropertyHandlerFlags::HAS_NO_SIDE_EFFECT.is_only_intercept_strings()
  );
  assert!(PropertyHandlerFlags::HAS_NO_SIDE_EFFECT.is_has_no_side_effect());

  assert_eq!(PropertyHandlerFlags::NONE, Default::default());

  let attr = PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS
    | PropertyHandlerFlags::HAS_NO_SIDE_EFFECT
    | PropertyHandlerFlags::NON_MASKING;
  assert!(!attr.is_none());
  assert!(attr.is_non_masking());
  assert!(attr.is_only_intercept_strings());
  assert!(attr.is_has_no_side_effect());
}
