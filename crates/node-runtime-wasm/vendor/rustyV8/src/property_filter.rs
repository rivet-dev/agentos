#[repr(C)]
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct PropertyFilter(u32);

impl PropertyFilter {
  pub const ALL_PROPERTIES: PropertyFilter = PropertyFilter(0);

  pub const ONLY_WRITABLE: PropertyFilter = PropertyFilter(1 << 0);

  pub const ONLY_ENUMERABLE: PropertyFilter = PropertyFilter(1 << 1);

  pub const ONLY_CONFIGURABLE: PropertyFilter = PropertyFilter(1 << 2);

  pub const SKIP_STRINGS: PropertyFilter = PropertyFilter(1 << 3);

  pub const SKIP_SYMBOLS: PropertyFilter = PropertyFilter(1 << 4);

  /// Test if all property filters are set.
  #[inline(always)]
  pub fn is_all_properties(&self) -> bool {
    *self == Self::ALL_PROPERTIES
  }

  /// Test if the only-writable property filter is set.
  #[inline(always)]
  pub fn is_only_writable(&self) -> bool {
    self.has(Self::ONLY_WRITABLE)
  }

  /// Test if the only-enumerable property filter is set.
  #[inline(always)]
  pub fn is_only_enumerable(&self) -> bool {
    self.has(Self::ONLY_ENUMERABLE)
  }

  /// Test if the only-configurable property filter is set.
  #[inline(always)]
  pub fn is_only_configurable(&self) -> bool {
    self.has(Self::ONLY_CONFIGURABLE)
  }

  /// Test if the skip-strings property filter is set.
  #[inline(always)]
  pub fn is_skip_strings(&self) -> bool {
    self.has(Self::SKIP_STRINGS)
  }

  /// Test if the skip-symbols property filter is set.
  #[inline(always)]
  pub fn is_skip_symbols(&self) -> bool {
    self.has(Self::SKIP_SYMBOLS)
  }

  #[inline(always)]
  fn has(&self, that: Self) -> bool {
    let Self(lhs) = self;
    let Self(rhs) = that;
    0 != lhs & rhs
  }
}

// Identical to #[derive(Default)] but arguably clearer when made explicit.
impl Default for PropertyFilter {
  fn default() -> Self {
    Self::ALL_PROPERTIES
  }
}

impl std::ops::BitOr for PropertyFilter {
  type Output = Self;

  fn bitor(self, Self(rhs): Self) -> Self {
    let Self(lhs) = self;
    Self(lhs | rhs)
  }
}

#[test]
fn test_attr() {
  assert!(PropertyFilter::ALL_PROPERTIES.is_all_properties());
  assert!(!PropertyFilter::ALL_PROPERTIES.is_only_writable());
  assert!(!PropertyFilter::ALL_PROPERTIES.is_only_enumerable());
  assert!(!PropertyFilter::ALL_PROPERTIES.is_only_configurable());
  assert!(!PropertyFilter::ALL_PROPERTIES.is_skip_strings());
  assert!(!PropertyFilter::ALL_PROPERTIES.is_skip_symbols());

  assert!(!PropertyFilter::ONLY_WRITABLE.is_all_properties());
  assert!(PropertyFilter::ONLY_WRITABLE.is_only_writable());
  assert!(!PropertyFilter::ONLY_WRITABLE.is_only_enumerable());
  assert!(!PropertyFilter::ONLY_WRITABLE.is_only_configurable());
  assert!(!PropertyFilter::ONLY_WRITABLE.is_skip_strings());
  assert!(!PropertyFilter::ONLY_WRITABLE.is_skip_symbols());

  assert!(!PropertyFilter::ONLY_ENUMERABLE.is_all_properties());
  assert!(!PropertyFilter::ONLY_ENUMERABLE.is_only_writable());
  assert!(PropertyFilter::ONLY_ENUMERABLE.is_only_enumerable());
  assert!(!PropertyFilter::ONLY_ENUMERABLE.is_only_configurable());
  assert!(!PropertyFilter::ONLY_ENUMERABLE.is_skip_strings());
  assert!(!PropertyFilter::ONLY_ENUMERABLE.is_skip_symbols());

  assert!(!PropertyFilter::ONLY_CONFIGURABLE.is_all_properties());
  assert!(!PropertyFilter::ONLY_CONFIGURABLE.is_only_writable());
  assert!(!PropertyFilter::ONLY_CONFIGURABLE.is_only_enumerable());
  assert!(PropertyFilter::ONLY_CONFIGURABLE.is_only_configurable());
  assert!(!PropertyFilter::ONLY_CONFIGURABLE.is_skip_strings());
  assert!(!PropertyFilter::ONLY_CONFIGURABLE.is_skip_symbols());

  assert!(!PropertyFilter::SKIP_STRINGS.is_all_properties());
  assert!(!PropertyFilter::SKIP_STRINGS.is_only_writable());
  assert!(!PropertyFilter::SKIP_STRINGS.is_only_enumerable());
  assert!(!PropertyFilter::SKIP_STRINGS.is_only_configurable());
  assert!(PropertyFilter::SKIP_STRINGS.is_skip_strings());
  assert!(!PropertyFilter::SKIP_STRINGS.is_skip_symbols());

  assert!(!PropertyFilter::SKIP_SYMBOLS.is_all_properties());
  assert!(!PropertyFilter::SKIP_SYMBOLS.is_only_writable());
  assert!(!PropertyFilter::SKIP_SYMBOLS.is_only_enumerable());
  assert!(!PropertyFilter::SKIP_SYMBOLS.is_only_configurable());
  assert!(!PropertyFilter::SKIP_SYMBOLS.is_skip_strings());
  assert!(PropertyFilter::SKIP_SYMBOLS.is_skip_symbols());

  assert_eq!(PropertyFilter::ALL_PROPERTIES, Default::default());
  assert_eq!(
    PropertyFilter::ONLY_WRITABLE,
    PropertyFilter::ALL_PROPERTIES | PropertyFilter::ONLY_WRITABLE
  );

  let attr = PropertyFilter::ONLY_WRITABLE
    | PropertyFilter::ONLY_WRITABLE
    | PropertyFilter::SKIP_STRINGS;
  assert!(!attr.is_all_properties());
  assert!(attr.is_only_writable());
  assert!(!attr.is_only_enumerable());
  assert!(!attr.is_only_configurable());
  assert!(attr.is_skip_strings());
  assert!(!attr.is_skip_symbols());
}
