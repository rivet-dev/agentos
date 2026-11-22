use crate::PropertyFilter;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum KeyConversionMode {
  /// kConvertToString will convert integer indices to strings.
  ConvertToString,
  /// kKeepNumbers will return numbers for integer indices.
  KeepNumbers,
  NoNumbers,
}

/// Keys/Properties filter enums:
///
/// KeyCollectionMode limits the range of collected properties. kOwnOnly limits
/// the collected properties to the given Object only. kIncludesPrototypes will
/// include all keys of the objects's prototype chain as well.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum KeyCollectionMode {
  /// OwnOnly limits the collected properties to the given Object only.
  OwnOnly,
  /// kIncludesPrototypes will include all keys of the objects's prototype chain
  /// as well.
  IncludePrototypes,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum IndexFilter {
  /// kIncludesIndices allows for integer indices to be collected.
  IncludeIndices,
  /// kSkipIndices will exclude integer indices from being collected.
  SkipIndices,
}

pub struct GetPropertyNamesArgs {
  pub mode: KeyCollectionMode,
  pub property_filter: PropertyFilter,
  pub index_filter: IndexFilter,
  pub key_conversion: KeyConversionMode,
}

impl Default for GetPropertyNamesArgs {
  fn default() -> Self {
    GetPropertyNamesArgs {
      mode: KeyCollectionMode::IncludePrototypes,
      property_filter: PropertyFilter::ONLY_ENUMERABLE
        | PropertyFilter::SKIP_SYMBOLS,
      index_filter: IndexFilter::IncludeIndices,
      key_conversion: KeyConversionMode::KeepNumbers,
    }
  }
}

pub struct GetPropertyNamesArgsBuilder {
  mode: KeyCollectionMode,
  property_filter: PropertyFilter,
  index_filter: IndexFilter,
  key_conversion: KeyConversionMode,
}

impl Default for GetPropertyNamesArgsBuilder {
  fn default() -> Self {
    Self::new()
  }
}

impl GetPropertyNamesArgsBuilder {
  #[inline(always)]
  pub fn new() -> Self {
    Self {
      mode: KeyCollectionMode::IncludePrototypes,
      property_filter: PropertyFilter::ONLY_ENUMERABLE
        | PropertyFilter::SKIP_SYMBOLS,
      index_filter: IndexFilter::IncludeIndices,
      key_conversion: KeyConversionMode::KeepNumbers,
    }
  }

  #[inline(always)]
  pub fn build(&self) -> GetPropertyNamesArgs {
    GetPropertyNamesArgs {
      mode: self.mode,
      property_filter: self.property_filter,
      index_filter: self.index_filter,
      key_conversion: self.key_conversion,
    }
  }

  #[inline(always)]
  pub fn mode(
    &mut self,
    mode: KeyCollectionMode,
  ) -> &mut GetPropertyNamesArgsBuilder {
    self.mode = mode;
    self
  }

  #[inline(always)]
  pub fn property_filter(
    &mut self,
    property_filter: PropertyFilter,
  ) -> &mut GetPropertyNamesArgsBuilder {
    self.property_filter = property_filter;
    self
  }

  #[inline(always)]
  pub fn index_filter(
    &mut self,
    index_filter: IndexFilter,
  ) -> &mut GetPropertyNamesArgsBuilder {
    self.index_filter = index_filter;
    self
  }

  #[inline(always)]
  pub fn key_conversion(
    &mut self,
    key_conversion: KeyConversionMode,
  ) -> &mut Self {
    self.key_conversion = key_conversion;
    self
  }
}
