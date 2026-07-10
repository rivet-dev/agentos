#include "edge_intl.h"

#include <algorithm>
#include <cstdint>
#include <mutex>
#include <string>
#include <unordered_set>
#include <vector>

// ICU is built with U_DISABLE_RENAMING=1 (see cmake/EdgeICU.cmake); consumers
// must match to avoid referencing versioned symbols (e.g. ulistfmt_open_78).
#define U_DISABLE_RENAMING 1
#include <unicode/ubrk.h>
#include <unicode/ucal.h>
#include <unicode/ucol.h>
#include <unicode/ucurr.h>
#include <unicode/udat.h>
#include <unicode/udatpg.h>
#include <unicode/ufieldpositer.h>
#include <unicode/uldnames.h>
#include <unicode/uloc.h>
#include <unicode/ulistformatter.h>
#include <unicode/unum.h>
#include <unicode/unumberformatter.h>
#include <unicode/upluralrules.h>
#include <unicode/ureldatefmt.h>
#include <unicode/uformattedvalue.h>
#include <unicode/ustring.h>
#include <unicode/utypes.h>

namespace {

// ---------------------------------------------------------------------------
// Shared N-API + ICU harness (reused by every constructor below)
// ---------------------------------------------------------------------------

void SetError(std::string* error_out, const std::string& message) {
  if (error_out != nullptr) *error_out = message;
}

napi_value Undefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

// Throws a JS RangeError and returns nullptr, matching ECMA-402 which reports
// invalid options/locales as RangeError.
napi_value ThrowRange(napi_env env, const std::string& message) {
  // Must be napi_throw_range_error (throws a real RangeError); napi_throw_error's
  // first arg is an error *code*, which would yield a plain Error instead.
  napi_throw_range_error(env, nullptr, message.c_str());
  return nullptr;
}

napi_value ThrowType(napi_env env, const std::string& message) {
  napi_throw_type_error(env, nullptr, message.c_str());
  return nullptr;
}

// JS string/coercible -> UTF-8 std::string. Returns fallback when absent.
std::string ValueToString(napi_env env, napi_value value, const std::string& fallback = "") {
  if (value == nullptr) return fallback;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined || type == napi_null) {
    return fallback;
  }
  napi_value str = value;
  if (type != napi_string && napi_coerce_to_string(env, value, &str) != napi_ok) {
    return fallback;
  }
  size_t len = 0;
  if (napi_get_value_string_utf8(env, str, nullptr, 0, &len) != napi_ok) return fallback;
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, str, out.data(), out.size(), &copied) != napi_ok) {
    return fallback;
  }
  out.resize(copied);
  return out;
}

napi_value MakeString(napi_env env, const std::string& value) {
  napi_value out = nullptr;
  napi_create_string_utf8(env, value.c_str(), value.size(), &out);
  return out;
}

napi_value GetNamed(napi_env env, napi_value object, const char* name) {
  if (object == nullptr) return nullptr;
  bool has = false;
  if (napi_has_named_property(env, object, name, &has) != napi_ok || !has) return nullptr;
  napi_value out = nullptr;
  if (napi_get_named_property(env, object, name, &out) != napi_ok) return nullptr;
  return out;
}

// Reads an enumerated string option, returning `fallback` when unset. If a value
// is present but not in `allowed`, returns "" so the caller can raise RangeError.
std::string GetStringOption(napi_env env,
                            napi_value options,
                            const char* name,
                            const std::vector<std::string>& allowed,
                            const std::string& fallback) {
  napi_value value = GetNamed(env, options, name);
  if (value == nullptr) return fallback;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined) return fallback;
  const std::string got = ValueToString(env, value, "");
  for (const std::string& candidate : allowed) {
    if (candidate == got) return got;
  }
  return "";  // present-but-invalid sentinel
}

// Reads a free-form (non-enumerated) string option, e.g. a currency code.
std::string GetRawStringOption(napi_env env, napi_value options, const char* name) {
  napi_value value = GetNamed(env, options, name);
  if (value == nullptr) return "";
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined) return "";
  return ValueToString(env, value, "");
}

// Reads an integer option; returns false when absent/unreadable.
bool GetIntOption(napi_env env, napi_value options, const char* name, int32_t* out) {
  napi_value value = GetNamed(env, options, name);
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined) return false;
  napi_value num = value;
  if (type != napi_number && napi_coerce_to_number(env, value, &num) != napi_ok) return false;
  double d = 0;
  if (napi_get_value_double(env, num, &d) != napi_ok) return false;
  *out = static_cast<int32_t>(d);
  return true;
}

// Reads a boolean option, defaulting when absent. ECMA-402 also allows string
// values for useGrouping; a present non-false value is treated as true.
bool GetBoolOptionDefault(napi_env env, napi_value options, const char* name, bool fallback) {
  napi_value value = GetNamed(env, options, name);
  if (value == nullptr) return fallback;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined) return fallback;
  if (type == napi_boolean) {
    bool out = fallback;
    return napi_get_value_bool(env, value, &out) == napi_ok ? out : fallback;
  }
  bool coerced = fallback;
  napi_value b = nullptr;
  if (napi_coerce_to_bool(env, value, &b) == napi_ok && b != nullptr &&
      napi_get_value_bool(env, b, &coerced) == napi_ok) {
    return coerced;
  }
  return fallback;
}

bool DefineMethod(napi_env env, napi_value object, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  return napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
         napi_set_named_property(env, object, name, fn) == napi_ok;
}

// ECMA-402 exposes Collator#compare and {Number,DateTime}Format#format as
// accessors returning a function bound to the instance, so they work when passed
// detached (e.g. arr.sort(collator.compare)). We approximate that by binding the
// prototype method to the instance and installing it as an own property at
// construction. The bound function's [[BoundThis]] also keeps the instance (and
// thus its ICU state) alive for the bound function's lifetime.
void BindOwnMethod(napi_env env, napi_value instance, const char* name) {
  napi_value method = nullptr;
  if (napi_get_named_property(env, instance, name, &method) != napi_ok || !IsFunction(env, method)) return;
  napi_value bind = nullptr;
  if (napi_get_named_property(env, method, "bind", &bind) != napi_ok || !IsFunction(env, bind)) return;
  napi_value bound = nullptr;
  if (napi_call_function(env, method, bind, 1, &instance, &bound) != napi_ok) return;
  napi_set_named_property(env, instance, name, bound);
}

bool InstallToStringTag(napi_env env, napi_value prototype, const char* tag) {
  napi_value global = nullptr;
  napi_value symbol = nullptr;
  napi_value key = nullptr;
  napi_value tag_value = nullptr;
  if (napi_get_global(env, &global) != napi_ok ||
      napi_get_named_property(env, global, "Symbol", &symbol) != napi_ok ||
      napi_get_named_property(env, symbol, "toStringTag", &key) != napi_ok ||
      napi_create_string_utf8(env, tag, NAPI_AUTO_LENGTH, &tag_value) != napi_ok) {
    return false;
  }
  napi_property_descriptor desc = {};
  desc.name = key;
  desc.value = tag_value;
  desc.attributes = napi_configurable;
  return napi_define_properties(env, prototype, 1, &desc) == napi_ok;
}

// --- ICU string + locale conversions ---------------------------------------

std::u16string ToUChars(const std::string& utf8) {
  if (utf8.empty()) return std::u16string();
  int32_t cap = static_cast<int32_t>(utf8.size()) + 1;
  std::u16string out(static_cast<size_t>(cap), u'\0');
  int32_t len = 0;
  UErrorCode status = U_ZERO_ERROR;
  u_strFromUTF8(reinterpret_cast<UChar*>(out.data()), cap, &len,
                utf8.data(), static_cast<int32_t>(utf8.size()), &status);
  if (U_FAILURE(status)) return std::u16string();
  out.resize(static_cast<size_t>(len));
  return out;
}

std::string FromUChars(const UChar* buf, int32_t len) {
  if (buf == nullptr || len <= 0) return "";
  int32_t cap = len * 3 + 1;  // worst-case UTF-8 expansion for BMP text
  std::string out(static_cast<size_t>(cap), '\0');
  int32_t out_len = 0;
  UErrorCode status = U_ZERO_ERROR;
  u_strToUTF8(out.data(), cap, &out_len, buf, len, &status);
  if (U_FAILURE(status)) return "";
  out.resize(static_cast<size_t>(out_len));
  return out;
}

// Builds a {type, value} part object from a UTF-16 slice, shared by the
// formatToParts implementations. begin/end are UTF-16 code-unit offsets.
napi_value MakePart(napi_env env, const char* type, const std::u16string& src, int32_t begin, int32_t end) {
  napi_value part = nullptr;
  napi_create_object(env, &part);
  napi_set_named_property(env, part, "type", MakeString(env, type));
  std::string value = FromUChars(reinterpret_cast<const UChar*>(src.data() + begin), end - begin);
  napi_set_named_property(env, part, "value", MakeString(env, value));
  return part;
}

// Resolves the first requested locale (BCP-47) to an ICU locale id. Defaults to
// en-US. Never throws; unparseable tags fall back to the default.
std::string ResolveIcuLocale(napi_env env, napi_value locales) {
  std::string tag = "en-US";
  if (locales != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, locales, &is_array) == napi_ok && is_array) {
      uint32_t len = 0;
      napi_value first = nullptr;
      if (napi_get_array_length(env, locales, &len) == napi_ok && len > 0 &&
          napi_get_element(env, locales, 0, &first) == napi_ok) {
        tag = ValueToString(env, first, "en-US");
      }
    } else {
      napi_valuetype type = napi_undefined;
      if (napi_typeof(env, locales, &type) == napi_ok &&
          type != napi_undefined && type != napi_null) {
        tag = ValueToString(env, locales, "en-US");
      }
    }
  }
  char buf[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_forLanguageTag(tag.c_str(), buf, sizeof(buf), nullptr, &status);
  if (U_FAILURE(status) || len <= 0) return "en_US";
  return std::string(buf, static_cast<size_t>(len));
}

// ICU locale id -> canonical BCP-47 tag (for resolvedOptions().locale).
std::string IcuLocaleToBcp47(const std::string& icu_locale) {
  char buf[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_toLanguageTag(icu_locale.c_str(), buf, sizeof(buf), true, &status);
  if (U_FAILURE(status) || len <= 0) return "en-US";
  return std::string(buf, static_cast<size_t>(len));
}

// The set of language subtags ICU actually has data for (computed once). Used to
// approximate ECMA-402 "lookup" matching in supportedLocalesOf.
const std::unordered_set<std::string>& AvailableLanguages() {
  static std::unordered_set<std::string> langs;
  static std::once_flag once;
  std::call_once(once, []() {
    int32_t n = uloc_countAvailable();
    for (int32_t i = 0; i < n; i++) {
      const char* id = uloc_getAvailable(i);
      if (id == nullptr) continue;
      std::string s(id);
      size_t underscore = s.find('_');
      langs.insert(underscore == std::string::npos ? s : s.substr(0, underscore));
    }
  });
  return langs;
}

// True when ICU has data for the locale's language subtag (lookup-style match).
bool LocaleHasData(const std::string& icu_locale) {
  char lang[ULOC_LANG_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_getLanguage(icu_locale.c_str(), lang, sizeof(lang), &status);
  if (U_FAILURE(status) || len <= 0) return false;
  return AvailableLanguages().count(std::string(lang, static_cast<size_t>(len))) > 0;
}

// Generic Intl static supportedLocalesOf: returns the requested locales (canonicalized)
// for which ICU actually has data. Never throws on valid input.
napi_value SupportedLocalesOf(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return Undefined(env);

  std::vector<std::string> requested;
  if (argc > 0 && argv[0] != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, argv[0], &is_array) == napi_ok && is_array) {
      uint32_t len = 0;
      napi_get_array_length(env, argv[0], &len);
      for (uint32_t i = 0; i < len; i++) {
        napi_value el = nullptr;
        if (napi_get_element(env, argv[0], i, &el) == napi_ok) {
          requested.push_back(ValueToString(env, el, ""));
        }
      }
    } else {
      napi_valuetype type = napi_undefined;
      if (napi_typeof(env, argv[0], &type) == napi_ok &&
          type != napi_undefined && type != napi_null) {
        requested.push_back(ValueToString(env, argv[0], ""));
      }
    }
  }

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  for (const std::string& tag : requested) {
    if (tag.empty()) continue;
    char buf[ULOC_FULLNAME_CAPACITY];
    UErrorCode status = U_ZERO_ERROR;
    int32_t len = uloc_forLanguageTag(tag.c_str(), buf, sizeof(buf), nullptr, &status);
    if (U_FAILURE(status) || len <= 0) continue;
    const std::string icu_locale(buf, static_cast<size_t>(len));
    if (!LocaleHasData(icu_locale)) continue;
    napi_set_element(env, out, idx++, MakeString(env, IcuLocaleToBcp47(icu_locale)));
  }
  return out;
}

// Reads a JS array of strings into a UTF-8 vector. Returns false if `value` is
// not an array (ECMA-402 accepts any iterable; arrays cover the real call sites).
bool ReadStringArray(napi_env env, napi_value value, std::vector<std::string>* out) {
  bool is_array = false;
  if (value == nullptr || napi_is_array(env, value, &is_array) != napi_ok || !is_array) {
    return false;
  }
  uint32_t len = 0;
  if (napi_get_array_length(env, value, &len) != napi_ok) return false;
  out->reserve(len);
  for (uint32_t i = 0; i < len; i++) {
    napi_value el = nullptr;
    if (napi_get_element(env, value, i, &el) != napi_ok) return false;
    out->push_back(ValueToString(env, el, ""));
  }
  return true;
}

// True when Intl already exposes a usable (function) member of this name. Used
// to leave the V8 provider's native ECMA-402 surface untouched: we only fill in
// constructors the engine lacks (i.e. on the QuickJS provider).
bool IntlHasUsable(napi_env env, napi_value intl, const char* name) {
  return IsFunction(env, GetNamed(env, intl, name));
}

// Installs a constructor built from a callback + prototype method table, wires
// the static supportedLocalesOf, the Symbol.toStringTag, and attaches it to Intl.
// No-op (success) when Intl already has a usable constructor of this name.
bool InstallConstructor(napi_env env,
                        napi_value intl,
                        const char* name,
                        napi_callback constructor,
                        const napi_property_descriptor* methods,
                        size_t method_count,
                        const char* tag,
                        std::string* error_out) {
  if (IntlHasUsable(env, intl, name)) return true;
  napi_value ctor = nullptr;
  napi_status status = napi_define_class(env, name, NAPI_AUTO_LENGTH, constructor, nullptr,
                                         method_count, methods, &ctor);
  if (status != napi_ok || ctor == nullptr) {
    SetError(error_out, std::string("Failed to define Intl.") + name);
    return false;
  }
  DefineMethod(env, ctor, "supportedLocalesOf", SupportedLocalesOf);
  napi_value prototype = nullptr;
  if (napi_get_named_property(env, ctor, "prototype", &prototype) == napi_ok && prototype != nullptr) {
    InstallToStringTag(env, prototype, tag);
  }
  if (napi_set_named_property(env, intl, name, ctor) != napi_ok) {
    SetError(error_out, std::string("Failed to install Intl.") + name);
    return false;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Intl.ListFormat  (ulistfmt_*)
// ---------------------------------------------------------------------------

struct ListFormatState {
  std::string locale;  // resolved ICU locale id
  std::string type;    // "conjunction" | "disjunction" | "unit"
  std::string style;   // "long" | "short" | "narrow"
  UListFormatter* fmt = nullptr;
};

void ListFormatFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  auto* state = static_cast<ListFormatState*>(data);
  if (state != nullptr) {
    if (state->fmt != nullptr) ulistfmt_close(state->fmt);
    delete state;
  }
}

ListFormatState* UnwrapListFormat(napi_env env, napi_value this_arg) {
  void* data = nullptr;
  if (this_arg == nullptr || napi_unwrap(env, this_arg, &data) != napi_ok) return nullptr;
  return static_cast<ListFormatState*>(data);
}

UListFormatterType ListTypeFromString(const std::string& type) {
  if (type == "disjunction") return ULISTFMT_TYPE_OR;
  if (type == "unit") return ULISTFMT_TYPE_UNITS;
  return ULISTFMT_TYPE_AND;  // "conjunction" (default)
}

UListFormatterWidth ListWidthFromString(const std::string& style) {
  if (style == "short") return ULISTFMT_WIDTH_SHORT;
  if (style == "narrow") return ULISTFMT_WIDTH_NARROW;
  return ULISTFMT_WIDTH_WIDE;  // "long" (default)
}

napi_value ListFormatConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) {
    return ThrowType(env, "Constructor Intl.ListFormat requires 'new'");
  }

  auto* state = new ListFormatState();
  state->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;
  state->type = GetStringOption(env, options, "type", {"conjunction", "disjunction", "unit"}, "conjunction");
  state->style = GetStringOption(env, options, "style", {"long", "short", "narrow"}, "long");
  if (state->type.empty()) {
    delete state;
    return ThrowRange(env, "Value for Intl.ListFormat option 'type' is out of range");
  }
  if (state->style.empty()) {
    delete state;
    return ThrowRange(env, "Value for Intl.ListFormat option 'style' is out of range");
  }

  UErrorCode status = U_ZERO_ERROR;
  state->fmt = ulistfmt_openForType(state->locale.c_str(),
                                    ListTypeFromString(state->type),
                                    ListWidthFromString(state->style), &status);
  if (U_FAILURE(status) || state->fmt == nullptr) {
    delete state;
    return ThrowRange(env, std::string("Failed to create Intl.ListFormat: ") + u_errorName(status));
  }

  if (napi_wrap(env, this_arg, state, ListFormatFinalize, nullptr, nullptr) != napi_ok) {
    ListFormatFinalize(env, state, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value ListFormatFormat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  ListFormatState* state = UnwrapListFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.ListFormat.prototype.format called on incompatible receiver");
  }

  std::vector<std::string> items;
  if (argc > 0 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    napi_typeof(env, argv[0], &type);
    if (type != napi_undefined && !ReadStringArray(env, argv[0], &items)) {
      return ThrowType(env, "Intl.ListFormat.prototype.format expects an array of strings");
    }
  }

  if (items.empty()) return MakeString(env, "");

  // Build the parallel UChar* / length arrays ICU expects. Backing storage must
  // outlive the ulistfmt_format call, so keep the u16 strings in a vector.
  std::vector<std::u16string> backing;
  backing.reserve(items.size());
  std::vector<const UChar*> ptrs;
  std::vector<int32_t> lengths;
  ptrs.reserve(items.size());
  lengths.reserve(items.size());
  for (const std::string& item : items) {
    backing.push_back(ToUChars(item));
    ptrs.push_back(reinterpret_cast<const UChar*>(backing.back().data()));
    lengths.push_back(static_cast<int32_t>(backing.back().size()));
  }

  UErrorCode status = U_ZERO_ERROR;
  int32_t needed = ulistfmt_format(state->fmt, ptrs.data(), lengths.data(),
                                   static_cast<int32_t>(items.size()), nullptr, 0, &status);
  if (status != U_BUFFER_OVERFLOW_ERROR && U_FAILURE(status)) {
    return ThrowRange(env, std::string("Intl.ListFormat.format failed: ") + u_errorName(status));
  }
  std::u16string result(static_cast<size_t>(needed), u'\0');
  status = U_ZERO_ERROR;
  ulistfmt_format(state->fmt, ptrs.data(), lengths.data(), static_cast<int32_t>(items.size()),
                  reinterpret_cast<UChar*>(result.data()), needed + 1, &status);
  if (U_FAILURE(status)) {
    return ThrowRange(env, std::string("Intl.ListFormat.format failed: ") + u_errorName(status));
  }
  return MakeString(env, FromUChars(reinterpret_cast<const UChar*>(result.data()),
                                    static_cast<int32_t>(result.size())));
}

// Shared: reads format()'s argument into parallel UChar*/length arrays (backed by
// `backing`, which must outlive the ICU call).
bool BuildListArrays(napi_env env, napi_value arg,
                     std::vector<std::u16string>* backing,
                     std::vector<const UChar*>* ptrs,
                     std::vector<int32_t>* lengths) {
  std::vector<std::string> items;
  if (arg != nullptr) {
    napi_valuetype type = napi_undefined;
    napi_typeof(env, arg, &type);
    if (type != napi_undefined && !ReadStringArray(env, arg, &items)) return false;
  }
  // Reserve up front: reallocation would move the (SSO) u16strings and dangle the
  // element pointers we hand to ICU.
  backing->reserve(items.size());
  for (const std::string& item : items) {
    backing->push_back(ToUChars(item));
    ptrs->push_back(reinterpret_cast<const UChar*>(backing->back().data()));
    lengths->push_back(static_cast<int32_t>(backing->back().size()));
  }
  return true;
}

napi_value ListFormatFormatToParts(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  ListFormatState* state = UnwrapListFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.ListFormat.prototype.formatToParts called on incompatible receiver");
  }
  std::vector<std::u16string> backing;
  std::vector<const UChar*> ptrs;
  std::vector<int32_t> lengths;
  if (!BuildListArrays(env, argc > 0 ? argv[0] : nullptr, &backing, &ptrs, &lengths)) {
    return ThrowType(env, "Intl.ListFormat.prototype.formatToParts expects an array of strings");
  }

  UErrorCode status = U_ZERO_ERROR;
  UFormattedList* fl = ulistfmt_openResult(&status);
  ulistfmt_formatStringsToResult(state->fmt, ptrs.data(), lengths.data(),
                                 static_cast<int32_t>(ptrs.size()), fl, &status);
  const UFormattedValue* fv = ulistfmt_resultAsValue(fl, &status);
  int32_t slen = 0;
  const UChar* sptr = U_SUCCESS(status) ? ufmtval_getString(fv, &slen, &status) : nullptr;
  if (U_FAILURE(status) || sptr == nullptr) {
    ulistfmt_closeResult(fl);
    return ThrowRange(env, std::string("ListFormat.formatToParts failed: ") + u_errorName(status));
  }
  const std::u16string text(reinterpret_cast<const char16_t*>(sptr), static_cast<size_t>(slen));

  // Collect element spans (category LIST, field ELEMENT); gaps become literals.
  struct Span { int32_t begin; int32_t end; };
  std::vector<Span> elements;
  UConstrainedFieldPosition* cfpos = ucfpos_open(&status);
  if (U_SUCCESS(status)) {
    ucfpos_constrainCategory(cfpos, UFIELD_CATEGORY_LIST, &status);
    while (ufmtval_nextPosition(fv, cfpos, &status) && U_SUCCESS(status)) {
      if (ucfpos_getField(cfpos, &status) != ULISTFMT_ELEMENT_FIELD) continue;
      int32_t begin = 0, end = 0;
      ucfpos_getIndexes(cfpos, &begin, &end, &status);
      elements.push_back({begin, end});
    }
  }
  ucfpos_close(cfpos);
  ulistfmt_closeResult(fl);
  std::sort(elements.begin(), elements.end(), [](const Span& a, const Span& b) { return a.begin < b.begin; });

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  int32_t cursor = 0;
  const int32_t total = static_cast<int32_t>(text.size());
  for (const Span& e : elements) {
    if (e.begin > cursor) napi_set_element(env, out, idx++, MakePart(env, "literal", text, cursor, e.begin));
    napi_set_element(env, out, idx++, MakePart(env, "element", text, e.begin, e.end));
    cursor = e.end;
  }
  if (cursor < total) napi_set_element(env, out, idx++, MakePart(env, "literal", text, cursor, total));
  return out;
}

napi_value ListFormatResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok) return nullptr;
  ListFormatState* state = UnwrapListFormat(env, this_arg);
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok) return Undefined(env);
  if (state != nullptr) {
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(state->locale)));
    napi_set_named_property(env, out, "type", MakeString(env, state->type));
    napi_set_named_property(env, out, "style", MakeString(env, state->style));
  }
  return out;
}

bool InstallListFormat(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"format", nullptr, ListFormatFormat, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"formatToParts", nullptr, ListFormatFormatToParts, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, ListFormatResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "ListFormat", ListFormatConstructor, methods,
                            sizeof(methods) / sizeof(methods[0]), "Intl.ListFormat", error_out);
}

// ---------------------------------------------------------------------------
// Intl.NumberFormat  (unumf_* modern number formatter)
// ---------------------------------------------------------------------------

struct NumberFormatState {
  std::string locale;    // resolved ICU locale id
  std::string style;     // "decimal" | "currency" | "percent" | "unit"
  std::string currency;  // ISO 4217, when style == currency
  std::string currency_display;  // "symbol" | "narrowSymbol" | "code" | "name"
  std::string currency_sign;     // "standard" | "accounting"
  std::string unit;      // when style == unit
  std::string unit_display;      // "short" | "narrow" | "long"
  std::string notation;          // "standard" | "scientific" | "engineering" | "compact"
  std::string compact_display;   // "short" | "long"
  std::string sign_display;      // "auto" | "never" | "always" | "exceptZero" | "negative"
  std::string rounding_mode;     // ECMA-402 roundingMode
  std::string trailing_zero_display;  // "auto" | "stripIfInteger"
  bool use_grouping = true;
  int32_t min_integer = -1;
  int32_t min_fraction = -1;  // -1 == unset (ICU default)
  int32_t max_fraction = -1;
  int32_t min_significant = -1;
  int32_t max_significant = -1;
  UNumberFormatter* fmt = nullptr;
};

void NumberFormatFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  auto* state = static_cast<NumberFormatState*>(data);
  if (state != nullptr) {
    if (state->fmt != nullptr) unumf_close(state->fmt);
    delete state;
  }
}

NumberFormatState* UnwrapNumberFormat(napi_env env, napi_value this_arg) {
  void* data = nullptr;
  if (this_arg == nullptr || napi_unwrap(env, this_arg, &data) != napi_ok) return nullptr;
  return static_cast<NumberFormatState*>(data);
}

// Builds an ICU concise number skeleton (unumberformatter.h) from resolved
// options. See https://unicode-org.github.io/icu/userguide/format_parse/numbers/skeletons
std::string BuildNumberSkeleton(const NumberFormatState& s) {
  std::string skel;
  auto add = [&](const std::string& token) {
    if (!skel.empty()) skel += " ";
    skel += token;
  };
  // Unit of measurement.
  if (s.style == "currency") {
    add("currency/" + s.currency);
    if (s.currency_display == "narrowSymbol") add("unit-width-narrow");
    else if (s.currency_display == "code") add("unit-width-iso-code");
    else if (s.currency_display == "name") add("unit-width-full-name");
  } else if (s.style == "percent") {
    add("percent");
    add("scale/100");  // ECMA-402 percent multiplies the input by 100
  } else if (s.style == "unit" && !s.unit.empty()) {
    add("unit/" + s.unit);
    if (s.unit_display == "long") add("unit-width-full-name");
    else if (s.unit_display == "narrow") add("unit-width-narrow");
    else add("unit-width-short");
  }
  // Precision: significant digits take precedence over fraction digits (ECMA-402).
  std::string prec;
  if (s.min_significant >= 0 || s.max_significant >= 0) {
    int32_t lo = s.min_significant < 0 ? 1 : s.min_significant;
    int32_t hi = s.max_significant < 0 ? 21 : s.max_significant;
    if (hi < lo) hi = lo;
    prec.assign(static_cast<size_t>(lo), '@');
    prec.append(static_cast<size_t>(hi - lo), '#');
  } else {
    // Fraction digits: explicit options, else ECMA-402 style defaults
    // (SetNumberFormatDigitOptions). currency -> the currency's minor-unit
    // digits (leave unset so ICU's currency skeleton applies them); percent ->
    // 0..0; decimal/unit -> 0..3.
    int32_t default_min = 0, default_max = 3;
    if (s.style == "currency") { default_min = -1; default_max = -1; }
    else if (s.style == "percent") { default_min = 0; default_max = 0; }
    int32_t lo = s.min_fraction >= 0 ? s.min_fraction : default_min;
    int32_t hi = s.max_fraction >= 0 ? s.max_fraction : default_max;
    if (lo >= 0 || hi >= 0) {
      if (lo < 0) lo = 0;
      if (hi < 0) hi = (lo > 3 ? lo : 3);
      if (hi < lo) hi = lo;
      if (hi == 0) {
        prec = "precision-integer";
      } else {
        prec = ".";
        prec.append(static_cast<size_t>(lo), '0');
        prec.append(static_cast<size_t>(hi - lo), '#');
      }
    }
  }
  if (!prec.empty()) {
    if (s.trailing_zero_display == "stripIfInteger") prec += "/w";  // strip fraction if whole
    add(prec);
  }
  if (s.min_integer > 1) add("integer-width/+" + std::string(static_cast<size_t>(s.min_integer), '0'));
  // Rounding mode.
  if (s.rounding_mode == "ceil") add("rounding-mode-ceiling");
  else if (s.rounding_mode == "floor") add("rounding-mode-floor");
  else if (s.rounding_mode == "expand") add("rounding-mode-up");
  else if (s.rounding_mode == "trunc") add("rounding-mode-down");
  else if (s.rounding_mode == "halfCeil") add("rounding-mode-half-ceiling");
  else if (s.rounding_mode == "halfFloor") add("rounding-mode-half-floor");
  else if (s.rounding_mode == "halfExpand") add("rounding-mode-half-up");
  else if (s.rounding_mode == "halfTrunc") add("rounding-mode-half-down");
  else if (s.rounding_mode == "halfEven") add("rounding-mode-half-even");
  // Notation.
  if (s.notation == "scientific") add("scientific");
  else if (s.notation == "engineering") add("engineering");
  else if (s.notation == "compact") add(s.compact_display == "long" ? "compact-long" : "compact-short");
  // Sign display.
  const bool acct = s.currency_sign == "accounting";
  if (s.sign_display == "never") add("sign-never");
  else if (s.sign_display == "always") add(acct ? "sign-accounting-always" : "sign-always");
  else if (s.sign_display == "exceptZero") add(acct ? "sign-accounting-except-zero" : "sign-except-zero");
  else if (s.sign_display == "negative") add(acct ? "sign-accounting-negative" : "sign-negative");
  else if (acct) add("sign-accounting");
  if (!s.use_grouping) add("group-off");
  return skel;
}

napi_value NumberFormatConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.NumberFormat requires 'new'");

  auto* state = new NumberFormatState();
  state->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;

  state->style = GetStringOption(env, options, "style", {"decimal", "currency", "percent", "unit"}, "decimal");
  if (state->style.empty()) {
    delete state;
    return ThrowRange(env, "Value for Intl.NumberFormat option 'style' is out of range");
  }
  if (state->style == "currency") {
    state->currency = GetRawStringOption(env, options, "currency");
    if (state->currency.empty()) {
      delete state;
      return ThrowType(env, "Currency code is required with currency style");
    }
    state->currency_display = GetStringOption(env, options, "currencyDisplay",
        {"symbol", "narrowSymbol", "code", "name"}, "symbol");
    state->currency_sign = GetStringOption(env, options, "currencySign", {"standard", "accounting"}, "standard");
  } else if (state->style == "unit") {
    state->unit = GetRawStringOption(env, options, "unit");
    if (state->unit.empty()) {
      delete state;
      return ThrowType(env, "Unit is required with unit style");
    }
    state->unit_display = GetStringOption(env, options, "unitDisplay", {"short", "narrow", "long"}, "short");
  }
  state->notation = GetStringOption(env, options, "notation",
      {"standard", "scientific", "engineering", "compact"}, "standard");
  state->compact_display = GetStringOption(env, options, "compactDisplay", {"short", "long"}, "short");
  state->sign_display = GetStringOption(env, options, "signDisplay",
      {"auto", "never", "always", "exceptZero", "negative"}, "auto");
  state->rounding_mode = GetStringOption(env, options, "roundingMode",
      {"ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand", "halfTrunc", "halfEven"},
      "halfExpand");
  state->trailing_zero_display = GetStringOption(env, options, "trailingZeroDisplay",
      {"auto", "stripIfInteger"}, "auto");
  state->use_grouping = GetBoolOptionDefault(env, options, "useGrouping", true);
  GetIntOption(env, options, "minimumIntegerDigits", &state->min_integer);
  GetIntOption(env, options, "minimumFractionDigits", &state->min_fraction);
  GetIntOption(env, options, "maximumFractionDigits", &state->max_fraction);
  GetIntOption(env, options, "minimumSignificantDigits", &state->min_significant);
  GetIntOption(env, options, "maximumSignificantDigits", &state->max_significant);

  const std::string skeleton = BuildNumberSkeleton(*state);
  const std::u16string uskel = ToUChars(skeleton);
  UErrorCode status = U_ZERO_ERROR;
  state->fmt = unumf_openForSkeletonAndLocale(
      reinterpret_cast<const UChar*>(uskel.data()), static_cast<int32_t>(uskel.size()),
      state->locale.c_str(), &status);
  if (U_FAILURE(status) || state->fmt == nullptr) {
    delete state;
    return ThrowRange(env, std::string("Failed to create Intl.NumberFormat: ") + u_errorName(status));
  }

  if (napi_wrap(env, this_arg, state, NumberFormatFinalize, nullptr, nullptr) != napi_ok) {
    NumberFormatFinalize(env, state, nullptr);
    return nullptr;
  }
  BindOwnMethod(env, this_arg, "format");
  return this_arg;
}

napi_value NumberFormatFormat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  NumberFormatState* state = UnwrapNumberFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.NumberFormat.prototype.format called on incompatible receiver");
  }

  double value = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype type = napi_undefined;
    napi_typeof(env, argv[0], &type);
    if (type != napi_number && napi_coerce_to_number(env, argv[0], &num) != napi_ok) {
      return MakeString(env, "NaN");
    }
    if (napi_get_value_double(env, num, &value) != napi_ok) return MakeString(env, "NaN");
  }

  UErrorCode status = U_ZERO_ERROR;
  UFormattedNumber* result = unumf_openResult(&status);
  if (U_FAILURE(status)) return ThrowRange(env, std::string("NumberFormat: ") + u_errorName(status));
  unumf_formatDouble(state->fmt, value, result, &status);
  if (U_FAILURE(status)) {
    unumf_closeResult(result);
    return ThrowRange(env, std::string("NumberFormat.format failed: ") + u_errorName(status));
  }

  int32_t needed = unumf_resultToString(result, nullptr, 0, &status);
  status = U_ZERO_ERROR;
  std::u16string out(static_cast<size_t>(needed), u'\0');
  unumf_resultToString(result, reinterpret_cast<UChar*>(out.data()), needed + 1, &status);
  unumf_closeResult(result);
  if (U_FAILURE(status)) {
    return ThrowRange(env, std::string("NumberFormat.format failed: ") + u_errorName(status));
  }
  return MakeString(env, FromUChars(reinterpret_cast<const UChar*>(out.data()),
                                    static_cast<int32_t>(out.size())));
}

const char* NumberFieldToPartType(int32_t field, const std::u16string& text, int32_t begin, int32_t end) {
  switch (field) {
    case UNUM_INTEGER_FIELD: return "integer";
    case UNUM_FRACTION_FIELD: return "fraction";
    case UNUM_DECIMAL_SEPARATOR_FIELD: return "decimal";
    case UNUM_GROUPING_SEPARATOR_FIELD: return "group";
    case UNUM_CURRENCY_FIELD: return "currency";
    case UNUM_PERCENT_FIELD: return "percentSign";
    case UNUM_PERMILL_FIELD: return "literal";
    case UNUM_EXPONENT_SYMBOL_FIELD: return "exponentSeparator";
    case UNUM_EXPONENT_SIGN_FIELD: return "exponentMinusSign";
    case UNUM_EXPONENT_FIELD: return "exponentInteger";
    case UNUM_MEASURE_UNIT_FIELD: return "unit";
    case UNUM_COMPACT_FIELD: return "compact";
    case UNUM_SIGN_FIELD: {
      // ECMA-402 splits the sign field into plusSign / minusSign.
      for (int32_t i = begin; i < end; i++) {
        if (text[i] == u'-' || text[i] == u'−') return "minusSign";
      }
      return "plusSign";
    }
    default: return "literal";
  }
}

napi_value NumberFormatFormatToParts(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  NumberFormatState* state = UnwrapNumberFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.NumberFormat.prototype.formatToParts called on incompatible receiver");
  }
  double value = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype t = napi_undefined;
    napi_typeof(env, argv[0], &t);
    if (t != napi_number) napi_coerce_to_number(env, argv[0], &num);
    napi_get_value_double(env, num, &value);
  }

  UErrorCode status = U_ZERO_ERROR;
  UFormattedNumber* result = unumf_openResult(&status);
  unumf_formatDouble(state->fmt, value, result, &status);
  int32_t needed = unumf_resultToString(result, nullptr, 0, &status);
  status = U_ZERO_ERROR;
  std::u16string text(static_cast<size_t>(needed), u'\0');
  unumf_resultToString(result, reinterpret_cast<UChar*>(text.data()), needed + 1, &status);
  text.resize(static_cast<size_t>(needed));

  UFieldPositionIterator* iter = ufieldpositer_open(&status);
  if (U_SUCCESS(status)) unumf_resultGetAllFieldPositions(result, iter, &status);
  unumf_closeResult(result);
  if (U_FAILURE(status)) {
    if (iter != nullptr) ufieldpositer_close(iter);
    return ThrowRange(env, std::string("NumberFormat.formatToParts failed: ") + u_errorName(status));
  }

  // ICU number fields nest (INTEGER spans the grouping separators that GROUP also
  // reports). Assign each code unit to the innermost (narrowest) covering field,
  // then coalesce equal-owner runs into parts. Unowned code units are "literal".
  const int32_t total = static_cast<int32_t>(text.size());
  std::vector<int32_t> owner(static_cast<size_t>(total), -1);
  std::vector<int32_t> width(static_cast<size_t>(total), total + 1);
  int32_t begin = 0, end = 0, id = 0;
  while ((id = ufieldpositer_next(iter, &begin, &end)) >= 0) {
    const int32_t w = end - begin;
    for (int32_t i = begin; i < end && i < total; i++) {
      if (w < width[i]) { width[i] = w; owner[i] = id; }
    }
  }
  ufieldpositer_close(iter);

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  int32_t i = 0;
  while (i < total) {
    int32_t j = i + 1;
    while (j < total && owner[static_cast<size_t>(j)] == owner[static_cast<size_t>(i)]) j++;
    const int32_t field = owner[static_cast<size_t>(i)];
    const char* type = field < 0 ? "literal" : NumberFieldToPartType(field, text, i, j);
    napi_set_element(env, out, idx++, MakePart(env, type, text, i, j));
    i = j;
  }
  return out;
}

napi_value NumberFormatResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok) return nullptr;
  NumberFormatState* state = UnwrapNumberFormat(env, this_arg);
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok) return Undefined(env);
  if (state != nullptr) {
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(state->locale)));
    napi_set_named_property(env, out, "numberingSystem", MakeString(env, "latn"));
    napi_set_named_property(env, out, "style", MakeString(env, state->style));
    auto put = [&](const char* k, const std::string& v) {
      if (!v.empty()) napi_set_named_property(env, out, k, MakeString(env, v));
    };
    if (!state->currency.empty()) {
      put("currency", state->currency);
      put("currencyDisplay", state->currency_display);
      put("currencySign", state->currency_sign);
    }
    if (!state->unit.empty()) {
      put("unit", state->unit);
      put("unitDisplay", state->unit_display);
    }
    put("notation", state->notation);
    if (state->notation == "compact") put("compactDisplay", state->compact_display);
    put("signDisplay", state->sign_display);
    put("roundingMode", state->rounding_mode);
    put("trailingZeroDisplay", state->trailing_zero_display);
    napi_value grouping = nullptr;
    napi_get_boolean(env, state->use_grouping, &grouping);
    napi_set_named_property(env, out, "useGrouping", grouping);
  }
  return out;
}

bool InstallNumberFormat(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"format", nullptr, NumberFormatFormat, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"formatToParts", nullptr, NumberFormatFormatToParts, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, NumberFormatResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "NumberFormat", NumberFormatConstructor, methods,
                            sizeof(methods) / sizeof(methods[0]), "Intl.NumberFormat", error_out);
}

// ---------------------------------------------------------------------------
// Intl.DateTimeFormat  (udat_* + udatpg_* pattern generator + ucal_*)
// ---------------------------------------------------------------------------

struct DateTimeFormatState {
  std::string locale;
  std::string time_zone;   // resolved IANA id (never empty)
  std::string calendar;    // e.g. "gregory"
  std::string hour_cycle;  // resolved: h11|h12|h23|h24 (empty if no hour)
  // Resolved component options (empty == not present), for resolvedOptions().
  std::string weekday, era, year, month, day, hour, minute, second, time_zone_name;
  std::string date_style, time_style;
  UDateFormat* fmt = nullptr;
};

void DateTimeFormatFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  auto* state = static_cast<DateTimeFormatState*>(data);
  if (state != nullptr) {
    if (state->fmt != nullptr) udat_close(state->fmt);
    delete state;
  }
}

DateTimeFormatState* UnwrapDateTimeFormat(napi_env env, napi_value this_arg) {
  void* data = nullptr;
  if (this_arg == nullptr || napi_unwrap(env, this_arg, &data) != napi_ok) return nullptr;
  return static_cast<DateTimeFormatState*>(data);
}

// Resolves the effective IANA time zone: the `timeZone` option if given, else the
// host default via ICU, falling back to a documented UTC when unavailable (e.g.
// in a sandbox with no host zone). Never returns empty.
std::string ResolveTimeZone(const std::string& requested) {
  if (!requested.empty()) return requested;
  UErrorCode status = U_ZERO_ERROR;
  UChar buf[128];
  int32_t len = ucal_getDefaultTimeZone(buf, 128, &status);
  if (U_FAILURE(status) || len <= 0) return "UTC";
  std::string zone = FromUChars(buf, len);
  if (zone.empty() || zone == "Etc/Unknown") return "UTC";
  return zone;
}

// Picks the skeleton hour letter per ECMA-402 hour12 / hourCycle. 'j' means
// "locale's preferred cycle"; explicit options override it. Also records the
// resolved hourCycle string on the state.
std::string HourSkeleton(const std::string& hour,
                         const std::string& hour_cycle,
                         bool hour12_set,
                         bool hour12,
                         std::string* resolved_cycle) {
  char c = 'j';
  if (hour_cycle == "h11") { c = 'K'; *resolved_cycle = "h11"; }
  else if (hour_cycle == "h12") { c = 'h'; *resolved_cycle = "h12"; }
  else if (hour_cycle == "h23") { c = 'H'; *resolved_cycle = "h23"; }
  else if (hour_cycle == "h24") { c = 'k'; *resolved_cycle = "h24"; }
  else if (hour12_set) {
    c = hour12 ? 'h' : 'H';
    *resolved_cycle = hour12 ? "h12" : "h23";
  }
  std::string s(1, c);
  if (hour == "2-digit") s += c;
  return s;
}

std::string BuildDateTimeSkeleton(DateTimeFormatState* s, bool hour12_set, bool hour12,
                                  const std::string& hour_cycle_opt) {
  std::string skel;
  auto rep = [](char c, int n) { return std::string(static_cast<size_t>(n), c); };
  if (s->weekday == "short") skel += "EEE";
  else if (s->weekday == "long") skel += "EEEE";
  else if (s->weekday == "narrow") skel += "EEEEE";
  if (s->era == "short") skel += "G";
  else if (s->era == "long") skel += "GGGG";
  else if (s->era == "narrow") skel += "GGGGG";
  if (s->year == "numeric") skel += "y";
  else if (s->year == "2-digit") skel += "yy";
  if (s->month == "numeric") skel += "M";
  else if (s->month == "2-digit") skel += "MM";
  else if (s->month == "short") skel += "MMM";
  else if (s->month == "long") skel += "MMMM";
  else if (s->month == "narrow") skel += "MMMMM";
  if (s->day == "numeric") skel += "d";
  else if (s->day == "2-digit") skel += "dd";
  if (!s->hour.empty()) {
    skel += HourSkeleton(s->hour, hour_cycle_opt, hour12_set, hour12, &s->hour_cycle);
  }
  if (s->minute == "numeric") skel += "m";
  else if (s->minute == "2-digit") skel += "mm";
  if (s->second == "numeric") skel += "s";
  else if (s->second == "2-digit") skel += "ss";
  if (s->time_zone_name == "short") skel += "z";
  else if (s->time_zone_name == "long") skel += "zzzz";
  else if (s->time_zone_name == "shortOffset") skel += "O";
  else if (s->time_zone_name == "longOffset") skel += "OOOO";
  else if (s->time_zone_name == "shortGeneric") skel += "v";
  else if (s->time_zone_name == "longGeneric") skel += "vvvv";
  (void)rep;
  return skel;
}

UDateFormatStyle DateStyleEnum(const std::string& style) {
  if (style == "full") return UDAT_FULL;
  if (style == "long") return UDAT_LONG;
  if (style == "medium") return UDAT_MEDIUM;
  if (style == "short") return UDAT_SHORT;
  return UDAT_NONE;
}

napi_value DateTimeFormatConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.DateTimeFormat requires 'new'");

  auto* state = new DateTimeFormatState();
  state->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;

  const std::vector<std::string> narrow = {"narrow", "short", "long"};
  const std::vector<std::string> numeric2 = {"numeric", "2-digit"};
  state->weekday = GetStringOption(env, options, "weekday", narrow, "");
  state->era = GetStringOption(env, options, "era", narrow, "");
  state->year = GetStringOption(env, options, "year", numeric2, "");
  state->month = GetStringOption(env, options, "month", {"numeric", "2-digit", "narrow", "short", "long"}, "");
  state->day = GetStringOption(env, options, "day", numeric2, "");
  state->hour = GetStringOption(env, options, "hour", numeric2, "");
  state->minute = GetStringOption(env, options, "minute", numeric2, "");
  state->second = GetStringOption(env, options, "second", numeric2, "");
  state->time_zone_name = GetStringOption(env, options, "timeZoneName",
      {"short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"}, "");
  state->date_style = GetStringOption(env, options, "dateStyle", {"full", "long", "medium", "short"}, "");
  state->time_style = GetStringOption(env, options, "timeStyle", {"full", "long", "medium", "short"}, "");

  bool hour12_set = GetNamed(env, options, "hour12") != nullptr;
  bool hour12 = hour12_set && GetBoolOptionDefault(env, options, "hour12", false);
  const std::string hour_cycle_opt = GetStringOption(env, options, "hourCycle", {"h11", "h12", "h23", "h24"}, "");

  state->time_zone = ResolveTimeZone(GetRawStringOption(env, options, "timeZone"));
  state->calendar = "gregory";
  const std::u16string tz = ToUChars(state->time_zone);

  UErrorCode status = U_ZERO_ERROR;
  if (!state->date_style.empty() || !state->time_style.empty()) {
    state->fmt = udat_open(DateStyleEnum(state->time_style), DateStyleEnum(state->date_style),
                           state->locale.c_str(), reinterpret_cast<const UChar*>(tz.data()),
                           static_cast<int32_t>(tz.size()), nullptr, 0, &status);
  } else {
    std::string skeleton = BuildDateTimeSkeleton(state, hour12_set, hour12, hour_cycle_opt);
    if (skeleton.empty()) {  // ECMA-402 default: year/month/day numeric
      state->year = state->month = state->day = "numeric";
      skeleton = "yMd";
    }
    const std::u16string uskel = ToUChars(skeleton);
    UDateTimePatternGenerator* dtpg = udatpg_open(state->locale.c_str(), &status);
    UChar pattern[256];
    int32_t plen = 0;
    if (U_SUCCESS(status)) {
      plen = udatpg_getBestPattern(dtpg, reinterpret_cast<const UChar*>(uskel.data()),
                                   static_cast<int32_t>(uskel.size()), pattern, 256, &status);
    }
    if (dtpg != nullptr) udatpg_close(dtpg);
    if (U_SUCCESS(status)) {
      state->fmt = udat_open(UDAT_PATTERN, UDAT_PATTERN, state->locale.c_str(),
                             reinterpret_cast<const UChar*>(tz.data()), static_cast<int32_t>(tz.size()),
                             pattern, plen, &status);
    }
  }
  if (U_FAILURE(status) || state->fmt == nullptr) {
    delete state;
    return ThrowRange(env, std::string("Failed to create Intl.DateTimeFormat: ") + u_errorName(status));
  }

  if (napi_wrap(env, this_arg, state, DateTimeFormatFinalize, nullptr, nullptr) != napi_ok) {
    DateTimeFormatFinalize(env, state, nullptr);
    return nullptr;
  }
  BindOwnMethod(env, this_arg, "format");
  return this_arg;
}

// Reads a UDate (ms since epoch) from format()'s argument: undefined -> now, a
// number -> itself, a Date-like -> getTime(), else coerce to number.
bool ReadUDate(napi_env env, size_t argc, napi_value arg, UDate* out) {
  if (argc == 0 || arg == nullptr) { *out = ucal_getNow(); return true; }
  napi_valuetype type = napi_undefined;
  napi_typeof(env, arg, &type);
  if (type == napi_undefined) { *out = ucal_getNow(); return true; }
  if (type == napi_object) {
    napi_value fn = GetNamed(env, arg, "getTime");
    napi_value result = nullptr;
    double d = 0;
    if (IsFunction(env, fn) && napi_call_function(env, arg, fn, 0, nullptr, &result) == napi_ok &&
        napi_get_value_double(env, result, &d) == napi_ok) {
      *out = d;
      return true;
    }
  }
  napi_value num = arg;
  double d = 0;
  if ((type == napi_number || napi_coerce_to_number(env, arg, &num) == napi_ok) &&
      napi_get_value_double(env, num, &d) == napi_ok) {
    *out = d;
    return true;
  }
  return false;
}

// CLDR 42+ (ICU 72+) inserts a narrow no-break space (U+202F) before the
// day-period (AM/PM) and in a few other date/time slots. Node patches its ICU
// data back to a plain space for compatibility; match that so formatted output
// (and the corresponding tests) line up with Node.
void NormalizeNarrowSpaces(std::u16string* s) {
  for (char16_t& c : *s) {
    if (c == u'\u202f') c = u'\u0020';  // narrow no-break space -> plain space
  }
}

napi_value DateTimeFormatFormat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  DateTimeFormatState* state = UnwrapDateTimeFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.DateTimeFormat.prototype.format called on incompatible receiver");
  }
  UDate when = 0;
  if (!ReadUDate(env, argc, argv[0], &when)) return MakeString(env, "Invalid Date");

  UErrorCode status = U_ZERO_ERROR;
  int32_t needed = udat_format(state->fmt, when, nullptr, 0, nullptr, &status);
  status = U_ZERO_ERROR;
  std::u16string out(static_cast<size_t>(needed), u'\0');
  udat_format(state->fmt, when, reinterpret_cast<UChar*>(out.data()), needed + 1, nullptr, &status);
  if (U_FAILURE(status)) {
    return ThrowRange(env, std::string("DateTimeFormat.format failed: ") + u_errorName(status));
  }
  NormalizeNarrowSpaces(&out);
  return MakeString(env, FromUChars(reinterpret_cast<const UChar*>(out.data()),
                                    static_cast<int32_t>(out.size())));
}

const char* DateFieldToPartType(int32_t field) {
  switch (field) {
    case UDAT_ERA_FIELD: return "era";
    case UDAT_YEAR_FIELD:
    case UDAT_YEAR_WOY_FIELD:
    case UDAT_EXTENDED_YEAR_FIELD: return "year";
    case UDAT_MONTH_FIELD:
    case UDAT_STANDALONE_MONTH_FIELD: return "month";
    case UDAT_DATE_FIELD: return "day";
    case UDAT_HOUR_OF_DAY1_FIELD:
    case UDAT_HOUR_OF_DAY0_FIELD:
    case UDAT_HOUR1_FIELD:
    case UDAT_HOUR0_FIELD: return "hour";
    case UDAT_MINUTE_FIELD: return "minute";
    case UDAT_SECOND_FIELD: return "second";
    case UDAT_FRACTIONAL_SECOND_FIELD: return "fractionalSecond";
    case UDAT_DAY_OF_WEEK_FIELD:
    case UDAT_STANDALONE_DAY_FIELD:
    case UDAT_DOW_LOCAL_FIELD: return "weekday";
    case UDAT_AM_PM_FIELD:
    case UDAT_AM_PM_MIDNIGHT_NOON_FIELD:
    case UDAT_FLEXIBLE_DAY_PERIOD_FIELD: return "dayPeriod";
    case UDAT_TIMEZONE_FIELD:
    case UDAT_TIMEZONE_RFC_FIELD:
    case UDAT_TIMEZONE_GENERIC_FIELD:
    case UDAT_TIMEZONE_SPECIAL_FIELD:
    case UDAT_TIMEZONE_LOCALIZED_GMT_OFFSET_FIELD:
    case UDAT_TIMEZONE_ISO_FIELD:
    case UDAT_TIMEZONE_ISO_LOCAL_FIELD: return "timeZoneName";
    default: return "literal";
  }
}

napi_value DateTimeFormatFormatToParts(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  DateTimeFormatState* state = UnwrapDateTimeFormat(env, this_arg);
  if (state == nullptr || state->fmt == nullptr) {
    return ThrowType(env, "Intl.DateTimeFormat.prototype.formatToParts called on incompatible receiver");
  }
  UDate when = 0;
  if (!ReadUDate(env, argc, argv[0], &when)) return Undefined(env);

  UErrorCode status = U_ZERO_ERROR;
  UFieldPositionIterator* iter = ufieldpositer_open(&status);
  if (U_FAILURE(status)) return Undefined(env);

  int32_t needed = udat_formatForFields(state->fmt, when, nullptr, 0, iter, &status);
  status = U_ZERO_ERROR;
  std::u16string text(static_cast<size_t>(needed), u'\0');
  udat_formatForFields(state->fmt, when, reinterpret_cast<UChar*>(text.data()), needed + 1, iter, &status);
  if (U_FAILURE(status)) {
    ufieldpositer_close(iter);
    return ThrowRange(env, std::string("DateTimeFormat.formatToParts failed: ") + u_errorName(status));
  }

  // Collect the located fields, then walk left-to-right emitting "literal" parts
  // for the gaps between them (ICU reports fields but not the connective text).
  struct Field { int32_t begin; int32_t end; int32_t id; };
  std::vector<Field> fields;
  int32_t begin = 0, end = 0, id = 0;
  while ((id = ufieldpositer_next(iter, &begin, &end)) >= 0) {
    fields.push_back({begin, end, id});
  }
  ufieldpositer_close(iter);
  std::sort(fields.begin(), fields.end(), [](const Field& a, const Field& b) { return a.begin < b.begin; });

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  int32_t cursor = 0;
  const int32_t total = static_cast<int32_t>(text.size());
  for (const Field& f : fields) {
    if (f.begin > cursor) {
      napi_set_element(env, out, idx++, MakePart(env, "literal", text, cursor, f.begin));
    }
    napi_set_element(env, out, idx++, MakePart(env, DateFieldToPartType(f.id), text, f.begin, f.end));
    cursor = f.end;
  }
  if (cursor < total) {
    napi_set_element(env, out, idx++, MakePart(env, "literal", text, cursor, total));
  }
  return out;
}

napi_value DateTimeFormatResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok) return nullptr;
  DateTimeFormatState* s = UnwrapDateTimeFormat(env, this_arg);
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok) return Undefined(env);
  if (s == nullptr) return out;
  napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
  napi_set_named_property(env, out, "calendar", MakeString(env, s->calendar));
  napi_set_named_property(env, out, "numberingSystem", MakeString(env, "latn"));
  napi_set_named_property(env, out, "timeZone", MakeString(env, s->time_zone));
  auto put = [&](const char* key, const std::string& val) {
    if (!val.empty()) napi_set_named_property(env, out, key, MakeString(env, val));
  };
  put("hourCycle", s->hour_cycle);
  put("weekday", s->weekday);
  put("era", s->era);
  put("year", s->year);
  put("month", s->month);
  put("day", s->day);
  put("hour", s->hour);
  put("minute", s->minute);
  put("second", s->second);
  put("timeZoneName", s->time_zone_name);
  put("dateStyle", s->date_style);
  put("timeStyle", s->time_style);
  return out;
}

bool InstallDateTimeFormat(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"format", nullptr, DateTimeFormatFormat, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"formatToParts", nullptr, DateTimeFormatFormatToParts, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, DateTimeFormatResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "DateTimeFormat", DateTimeFormatConstructor, methods,
                            sizeof(methods) / sizeof(methods[0]), "Intl.DateTimeFormat", error_out);
}

// ---------------------------------------------------------------------------
// Intl.PluralRules  (uplrules_*)
// ---------------------------------------------------------------------------

struct PluralRulesState {
  std::string locale;
  std::string type;  // "cardinal" | "ordinal"
  UPluralRules* rules = nullptr;
};

void PluralRulesFinalize(napi_env, void* data, void*) {
  auto* s = static_cast<PluralRulesState*>(data);
  if (s != nullptr) {
    if (s->rules != nullptr) uplrules_close(s->rules);
    delete s;
  }
}

napi_value PluralRulesConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.PluralRules requires 'new'");

  auto* s = new PluralRulesState();
  s->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  s->type = GetStringOption(env, argc > 1 ? argv[1] : nullptr, "type", {"cardinal", "ordinal"}, "cardinal");
  if (s->type.empty()) { delete s; return ThrowRange(env, "Invalid Intl.PluralRules option 'type'"); }
  UErrorCode status = U_ZERO_ERROR;
  s->rules = uplrules_openForType(s->locale.c_str(),
                                  s->type == "ordinal" ? UPLURAL_TYPE_ORDINAL : UPLURAL_TYPE_CARDINAL,
                                  &status);
  if (U_FAILURE(status) || s->rules == nullptr) {
    delete s;
    return ThrowRange(env, std::string("Failed to create Intl.PluralRules: ") + u_errorName(status));
  }
  if (napi_wrap(env, this_arg, s, PluralRulesFinalize, nullptr, nullptr) != napi_ok) {
    PluralRulesFinalize(env, s, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value PluralRulesSelect(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.PluralRules.prototype.select called on incompatible receiver");
  }
  auto* s = static_cast<PluralRulesState*>(data);
  double value = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype t = napi_undefined;
    napi_typeof(env, argv[0], &t);
    if (t != napi_number) napi_coerce_to_number(env, argv[0], &num);
    napi_get_value_double(env, num, &value);
  }
  UErrorCode status = U_ZERO_ERROR;
  UChar keyword[64];
  int32_t len = uplrules_select(s->rules, value, keyword, 64, &status);
  if (U_FAILURE(status)) return MakeString(env, "other");
  return MakeString(env, FromUChars(keyword, len));
}

napi_value PluralRulesResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  void* data = nullptr;
  napi_unwrap(env, this_arg, &data);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (data != nullptr) {
    auto* s = static_cast<PluralRulesState*>(data);
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
    napi_set_named_property(env, out, "type", MakeString(env, s->type));
  }
  return out;
}

bool InstallPluralRules(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"select", nullptr, PluralRulesSelect, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, PluralRulesResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "PluralRules", PluralRulesConstructor, methods, 2,
                            "Intl.PluralRules", error_out);
}

// ---------------------------------------------------------------------------
// Intl.Collator  (ucol_*)
// ---------------------------------------------------------------------------

struct CollatorState {
  std::string locale;
  bool numeric = false;
  bool ignore_punctuation = false;
  std::string sensitivity;
  UCollator* coll = nullptr;
};

void CollatorFinalize(napi_env, void* data, void*) {
  auto* s = static_cast<CollatorState*>(data);
  if (s != nullptr) {
    if (s->coll != nullptr) ucol_close(s->coll);
    delete s;
  }
}

napi_value CollatorConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.Collator requires 'new'");

  auto* s = new CollatorState();
  s->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;
  s->numeric = GetBoolOptionDefault(env, options, "numeric", false);
  s->ignore_punctuation = GetBoolOptionDefault(env, options, "ignorePunctuation", false);
  s->sensitivity = GetStringOption(env, options, "sensitivity", {"base", "accent", "case", "variant"}, "");

  UErrorCode status = U_ZERO_ERROR;
  s->coll = ucol_open(s->locale.c_str(), &status);
  if (U_FAILURE(status) || s->coll == nullptr) {
    delete s;
    return ThrowRange(env, std::string("Failed to create Intl.Collator: ") + u_errorName(status));
  }
  if (s->numeric) {
    UErrorCode a = U_ZERO_ERROR;
    ucol_setAttribute(s->coll, UCOL_NUMERIC_COLLATION, UCOL_ON, &a);
  }
  if (s->ignore_punctuation) {
    // Shift punctuation/whitespace below the variable top so it is ignored at
    // primary/secondary strength (ECMA-402 ignorePunctuation).
    UErrorCode a = U_ZERO_ERROR;
    ucol_setAttribute(s->coll, UCOL_ALTERNATE_HANDLING, UCOL_SHIFTED, &a);
  }
  if (s->sensitivity == "base") ucol_setStrength(s->coll, UCOL_PRIMARY);
  else if (s->sensitivity == "accent") ucol_setStrength(s->coll, UCOL_SECONDARY);
  else if (s->sensitivity == "case" || s->sensitivity == "variant") ucol_setStrength(s->coll, UCOL_TERTIARY);

  if (napi_wrap(env, this_arg, s, CollatorFinalize, nullptr, nullptr) != napi_ok) {
    CollatorFinalize(env, s, nullptr);
    return nullptr;
  }
  BindOwnMethod(env, this_arg, "compare");
  return this_arg;
}

napi_value CollatorCompare(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.Collator.prototype.compare called on incompatible receiver");
  }
  auto* s = static_cast<CollatorState*>(data);
  const std::u16string a = ToUChars(ValueToString(env, argc > 0 ? argv[0] : nullptr, ""));
  const std::u16string b = ToUChars(ValueToString(env, argc > 1 ? argv[1] : nullptr, ""));
  UCollationResult r = ucol_strcoll(s->coll,
                                    reinterpret_cast<const UChar*>(a.data()), static_cast<int32_t>(a.size()),
                                    reinterpret_cast<const UChar*>(b.data()), static_cast<int32_t>(b.size()));
  napi_value out = nullptr;
  napi_create_int32(env, r == UCOL_LESS ? -1 : (r == UCOL_GREATER ? 1 : 0), &out);
  return out;
}

napi_value CollatorResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  void* data = nullptr;
  napi_unwrap(env, this_arg, &data);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (data != nullptr) {
    auto* s = static_cast<CollatorState*>(data);
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
    napi_value numeric = nullptr;
    napi_get_boolean(env, s->numeric, &numeric);
    napi_set_named_property(env, out, "numeric", numeric);
    if (!s->sensitivity.empty()) {
      napi_set_named_property(env, out, "sensitivity", MakeString(env, s->sensitivity));
    }
  }
  return out;
}

bool InstallCollator(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"compare", nullptr, CollatorCompare, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, CollatorResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "Collator", CollatorConstructor, methods, 2,
                            "Intl.Collator", error_out);
}

// ---------------------------------------------------------------------------
// Intl.RelativeTimeFormat  (ureldatefmt_*)
// ---------------------------------------------------------------------------

struct RelativeTimeFormatState {
  std::string locale;
  std::string numeric;  // "always" | "auto"
  std::string style;    // "long" | "short" | "narrow"
  URelativeDateTimeFormatter* fmt = nullptr;
};

void RelativeTimeFormatFinalize(napi_env, void* data, void*) {
  auto* s = static_cast<RelativeTimeFormatState*>(data);
  if (s != nullptr) {
    if (s->fmt != nullptr) ureldatefmt_close(s->fmt);
    delete s;
  }
}

bool RelativeUnitFromString(const std::string& unit, URelativeDateTimeUnit* out) {
  std::string u = unit;
  if (!u.empty() && u.back() == 's') u.pop_back();  // accept plural forms
  if (u == "year") *out = UDAT_REL_UNIT_YEAR;
  else if (u == "quarter") *out = UDAT_REL_UNIT_QUARTER;
  else if (u == "month") *out = UDAT_REL_UNIT_MONTH;
  else if (u == "week") *out = UDAT_REL_UNIT_WEEK;
  else if (u == "day") *out = UDAT_REL_UNIT_DAY;
  else if (u == "hour") *out = UDAT_REL_UNIT_HOUR;
  else if (u == "minute") *out = UDAT_REL_UNIT_MINUTE;
  else if (u == "second") *out = UDAT_REL_UNIT_SECOND;
  else return false;
  return true;
}

napi_value RelativeTimeFormatConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.RelativeTimeFormat requires 'new'");

  auto* s = new RelativeTimeFormatState();
  s->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;
  s->numeric = GetStringOption(env, options, "numeric", {"always", "auto"}, "always");
  s->style = GetStringOption(env, options, "style", {"long", "short", "narrow"}, "long");
  if (s->numeric.empty() || s->style.empty()) {
    delete s;
    return ThrowRange(env, "Invalid Intl.RelativeTimeFormat option");
  }
  UDateRelativeDateTimeFormatterStyle width = UDAT_STYLE_LONG;
  if (s->style == "short") width = UDAT_STYLE_SHORT;
  else if (s->style == "narrow") width = UDAT_STYLE_NARROW;

  UErrorCode status = U_ZERO_ERROR;
  s->fmt = ureldatefmt_open(s->locale.c_str(), nullptr, width, UDISPCTX_CAPITALIZATION_NONE, &status);
  if (U_FAILURE(status) || s->fmt == nullptr) {
    delete s;
    return ThrowRange(env, std::string("Failed to create Intl.RelativeTimeFormat: ") + u_errorName(status));
  }
  if (napi_wrap(env, this_arg, s, RelativeTimeFormatFinalize, nullptr, nullptr) != napi_ok) {
    RelativeTimeFormatFinalize(env, s, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value RelativeTimeFormatFormat(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.RelativeTimeFormat.prototype.format called on incompatible receiver");
  }
  auto* s = static_cast<RelativeTimeFormatState*>(data);
  double value = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype t = napi_undefined;
    napi_typeof(env, argv[0], &t);
    if (t != napi_number) napi_coerce_to_number(env, argv[0], &num);
    napi_get_value_double(env, num, &value);
  }
  URelativeDateTimeUnit unit;
  if (!RelativeUnitFromString(ValueToString(env, argc > 1 ? argv[1] : nullptr, ""), &unit)) {
    return ThrowRange(env, "Invalid unit argument for Intl.RelativeTimeFormat.format()");
  }
  UErrorCode status = U_ZERO_ERROR;
  int32_t needed;
  std::u16string out(64, u'\0');
  if (s->numeric == "auto") {
    needed = ureldatefmt_format(s->fmt, value, unit, reinterpret_cast<UChar*>(out.data()),
                                static_cast<int32_t>(out.size()), &status);
  } else {
    needed = ureldatefmt_formatNumeric(s->fmt, value, unit, reinterpret_cast<UChar*>(out.data()),
                                       static_cast<int32_t>(out.size()), &status);
  }
  if (status == U_BUFFER_OVERFLOW_ERROR) {
    status = U_ZERO_ERROR;
    out.assign(static_cast<size_t>(needed), u'\0');
    if (s->numeric == "auto") {
      ureldatefmt_format(s->fmt, value, unit, reinterpret_cast<UChar*>(out.data()), needed + 1, &status);
    } else {
      ureldatefmt_formatNumeric(s->fmt, value, unit, reinterpret_cast<UChar*>(out.data()), needed + 1, &status);
    }
  } else if (U_SUCCESS(status)) {
    out.resize(static_cast<size_t>(needed));
  }
  if (U_FAILURE(status)) {
    return ThrowRange(env, std::string("RelativeTimeFormat.format failed: ") + u_errorName(status));
  }
  return MakeString(env, FromUChars(reinterpret_cast<const UChar*>(out.data()),
                                    static_cast<int32_t>(out.size())));
}

napi_value RelativeTimeFormatFormatToParts(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.RelativeTimeFormat.prototype.formatToParts called on incompatible receiver");
  }
  auto* s = static_cast<RelativeTimeFormatState*>(data);
  double value = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype t = napi_undefined;
    napi_typeof(env, argv[0], &t);
    if (t != napi_number) napi_coerce_to_number(env, argv[0], &num);
    napi_get_value_double(env, num, &value);
  }
  std::string unit_str = ValueToString(env, argc > 1 ? argv[1] : nullptr, "");
  URelativeDateTimeUnit unit;
  if (!RelativeUnitFromString(unit_str, &unit)) {
    return ThrowRange(env, "Invalid unit argument for Intl.RelativeTimeFormat.formatToParts()");
  }
  if (!unit_str.empty() && unit_str.back() == 's') unit_str.pop_back();  // singular for the part's `unit`

  UErrorCode status = U_ZERO_ERROR;
  UFormattedRelativeDateTime* result = ureldatefmt_openResult(&status);
  if (s->numeric == "auto") {
    ureldatefmt_formatToResult(s->fmt, value, unit, result, &status);
  } else {
    ureldatefmt_formatNumericToResult(s->fmt, value, unit, result, &status);
  }
  const UFormattedValue* fv = U_SUCCESS(status) ? ureldatefmt_resultAsValue(result, &status) : nullptr;
  int32_t slen = 0;
  const UChar* sptr = fv != nullptr ? ufmtval_getString(fv, &slen, &status) : nullptr;
  if (U_FAILURE(status) || sptr == nullptr) {
    ureldatefmt_closeResult(result);
    return ThrowRange(env, std::string("RelativeTimeFormat.formatToParts failed: ") + u_errorName(status));
  }
  const std::u16string text(reinterpret_cast<const char16_t*>(sptr), static_cast<size_t>(slen));
  const int32_t total = static_cast<int32_t>(text.size());

  // Assign each code unit to the innermost NUMBER-category field (if any).
  std::vector<int32_t> owner(static_cast<size_t>(total), -1);
  std::vector<int32_t> width(static_cast<size_t>(total), total + 1);
  UConstrainedFieldPosition* cfpos = ucfpos_open(&status);
  if (U_SUCCESS(status)) {
    ucfpos_constrainCategory(cfpos, UFIELD_CATEGORY_NUMBER, &status);
    while (ufmtval_nextPosition(fv, cfpos, &status) && U_SUCCESS(status)) {
      int32_t begin = 0, end = 0;
      ucfpos_getIndexes(cfpos, &begin, &end, &status);
      const int32_t id = ucfpos_getField(cfpos, &status);
      const int32_t w = end - begin;
      for (int32_t i = begin; i < end && i < total; i++) {
        if (w < width[static_cast<size_t>(i)]) { width[static_cast<size_t>(i)] = w; owner[static_cast<size_t>(i)] = id; }
      }
    }
  }
  ucfpos_close(cfpos);
  ureldatefmt_closeResult(result);

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  int32_t i = 0;
  napi_value unit_val = MakeString(env, unit_str);
  while (i < total) {
    int32_t j = i + 1;
    while (j < total && owner[static_cast<size_t>(j)] == owner[static_cast<size_t>(i)]) j++;
    const int32_t field = owner[static_cast<size_t>(i)];
    napi_value part;
    if (field < 0) {
      part = MakePart(env, "literal", text, i, j);
    } else {
      part = MakePart(env, NumberFieldToPartType(field, text, i, j), text, i, j);
      napi_set_named_property(env, part, "unit", unit_val);
    }
    napi_set_element(env, out, idx++, part);
    i = j;
  }
  return out;
}

napi_value RelativeTimeFormatResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  void* data = nullptr;
  napi_unwrap(env, this_arg, &data);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (data != nullptr) {
    auto* s = static_cast<RelativeTimeFormatState*>(data);
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
    napi_set_named_property(env, out, "style", MakeString(env, s->style));
    napi_set_named_property(env, out, "numeric", MakeString(env, s->numeric));
    napi_set_named_property(env, out, "numberingSystem", MakeString(env, "latn"));
  }
  return out;
}

bool InstallRelativeTimeFormat(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"format", nullptr, RelativeTimeFormatFormat, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"formatToParts", nullptr, RelativeTimeFormatFormatToParts, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, RelativeTimeFormatResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "RelativeTimeFormat", RelativeTimeFormatConstructor, methods, 3,
                            "Intl.RelativeTimeFormat", error_out);
}

// ---------------------------------------------------------------------------
// Intl.DisplayNames  (uldn_*)  -- language/region/script (currency deferred)
// ---------------------------------------------------------------------------

struct DisplayNamesState {
  std::string locale;
  std::string type;   // "language" | "region" | "script" | "currency"
  std::string style;  // "long" | "short" | "narrow"
  std::string fallback;
  ULocaleDisplayNames* ldn = nullptr;
};

void DisplayNamesFinalize(napi_env, void* data, void*) {
  auto* s = static_cast<DisplayNamesState*>(data);
  if (s != nullptr) {
    if (s->ldn != nullptr) uldn_close(s->ldn);
    delete s;
  }
}

napi_value DisplayNamesConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.DisplayNames requires 'new'");

  auto* s = new DisplayNamesState();
  s->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  napi_value options = argc > 1 ? argv[1] : nullptr;
  s->type = GetStringOption(env, options, "type", {"language", "region", "script", "currency", "calendar", "dateTimeField"}, "");
  if (s->type.empty()) { delete s; return ThrowType(env, "Intl.DisplayNames: option 'type' is required"); }
  s->style = GetStringOption(env, options, "style", {"long", "short", "narrow"}, "long");
  s->fallback = GetStringOption(env, options, "fallback", {"code", "none"}, "code");

  UErrorCode status = U_ZERO_ERROR;
  s->ldn = uldn_open(s->locale.c_str(), ULDN_STANDARD_NAMES, &status);
  if (U_FAILURE(status) || s->ldn == nullptr) {
    delete s;
    return ThrowRange(env, std::string("Failed to create Intl.DisplayNames: ") + u_errorName(status));
  }
  if (napi_wrap(env, this_arg, s, DisplayNamesFinalize, nullptr, nullptr) != napi_ok) {
    DisplayNamesFinalize(env, s, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value DisplayNamesOf(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.DisplayNames.prototype.of called on incompatible receiver");
  }
  auto* s = static_cast<DisplayNamesState*>(data);
  const std::string code = ValueToString(env, argc > 0 ? argv[0] : nullptr, "");

  UErrorCode status = U_ZERO_ERROR;
  UChar buf[256];
  int32_t len = 0;
  if (s->type == "region") {
    len = uldn_regionDisplayName(s->ldn, code.c_str(), buf, 256, &status);
  } else if (s->type == "script") {
    len = uldn_scriptDisplayName(s->ldn, code.c_str(), buf, 256, &status);
  } else if (s->type == "language") {
    len = uldn_languageDisplayName(s->ldn, code.c_str(), buf, 256, &status);
  } else if (s->type == "currency") {
    // ucurr_getName wants the ISO code as UChars; "narrow"/"short" -> symbol.
    const std::u16string ucode = ToUChars(code);
    UBool is_choice = false;
    int32_t name_len = 0;
    const UChar* name = ucurr_getName(reinterpret_cast<const UChar*>(ucode.data()), s->locale.c_str(),
                                      s->style == "long" ? UCURR_LONG_NAME : UCURR_SYMBOL_NAME,
                                      &is_choice, &name_len, &status);
    if (U_FAILURE(status) || name == nullptr || name_len <= 0) {
      return s->fallback == "none" ? Undefined(env) : MakeString(env, code);
    }
    return MakeString(env, FromUChars(name, name_len));
  } else if (s->type == "calendar") {
    len = uldn_keyValueDisplayName(s->ldn, "calendar", code.c_str(), buf, 256, &status);
  } else if (s->type == "dateTimeField") {
    UDateTimePatternField field;
    if (code == "era") field = UDATPG_ERA_FIELD;
    else if (code == "year") field = UDATPG_YEAR_FIELD;
    else if (code == "quarter") field = UDATPG_QUARTER_FIELD;
    else if (code == "month") field = UDATPG_MONTH_FIELD;
    else if (code == "weekOfYear") field = UDATPG_WEEK_OF_YEAR_FIELD;
    else if (code == "weekday") field = UDATPG_WEEKDAY_FIELD;
    else if (code == "day") field = UDATPG_DAY_FIELD;
    else if (code == "dayPeriod") field = UDATPG_DAYPERIOD_FIELD;
    else if (code == "hour") field = UDATPG_HOUR_FIELD;
    else if (code == "minute") field = UDATPG_MINUTE_FIELD;
    else if (code == "second") field = UDATPG_SECOND_FIELD;
    else if (code == "timeZoneName") field = UDATPG_ZONE_FIELD;
    else return s->fallback == "none" ? Undefined(env) : MakeString(env, code);
    UDateTimePGDisplayWidth width = s->style == "short" ? UDATPG_ABBREVIATED
        : (s->style == "narrow" ? UDATPG_NARROW : UDATPG_WIDE);
    UDateTimePatternGenerator* dtpg = udatpg_open(s->locale.c_str(), &status);
    if (U_SUCCESS(status)) len = udatpg_getFieldDisplayName(dtpg, field, width, buf, 256, &status);
    if (dtpg != nullptr) udatpg_close(dtpg);
  } else {
    return s->fallback == "none" ? Undefined(env) : MakeString(env, code);
  }
  if (U_FAILURE(status) || len <= 0) {
    return s->fallback == "none" ? Undefined(env) : MakeString(env, code);
  }
  return MakeString(env, FromUChars(buf, len));
}

napi_value DisplayNamesResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  void* data = nullptr;
  napi_unwrap(env, this_arg, &data);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (data != nullptr) {
    auto* s = static_cast<DisplayNamesState*>(data);
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
    napi_set_named_property(env, out, "type", MakeString(env, s->type));
    napi_set_named_property(env, out, "style", MakeString(env, s->style));
    napi_set_named_property(env, out, "fallback", MakeString(env, s->fallback));
  }
  return out;
}

bool InstallDisplayNames(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"of", nullptr, DisplayNamesOf, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, DisplayNamesResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "DisplayNames", DisplayNamesConstructor, methods, 2,
                            "Intl.DisplayNames", error_out);
}

// ---------------------------------------------------------------------------
// Intl.Locale  (uloc_*) -- a locale object with accessors, not a formatter
// ---------------------------------------------------------------------------

std::string* UnwrapLocale(napi_env env, napi_value this_arg) {
  void* data = nullptr;
  if (this_arg == nullptr || napi_unwrap(env, this_arg, &data) != napi_ok) return nullptr;
  return static_cast<std::string*>(data);  // holds the ICU locale id
}

void LocaleFinalize(napi_env, void* data, void*) { delete static_cast<std::string*>(data); }

// Reads a single uloc_* subtag (language/script/country) into UTF-8.
std::string LocaleSubtag(const std::string& icu_locale,
                         int32_t (*fn)(const char*, char*, int32_t, UErrorCode*)) {
  char buf[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = fn(icu_locale.c_str(), buf, sizeof(buf), &status);
  if (U_FAILURE(status) || len <= 0) return "";
  return std::string(buf, static_cast<size_t>(len));
}

napi_value LocaleConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.Locale requires 'new'");
  if (argc < 1) return ThrowType(env, "First argument to Intl.Locale must be defined");
  auto* state = new std::string(ResolveIcuLocale(env, argv[0]));
  if (napi_wrap(env, this_arg, state, LocaleFinalize, nullptr, nullptr) != napi_ok) {
    delete state;
    return nullptr;
  }
  return this_arg;
}

// Reads a Unicode ("-u-") extension keyword (e.g. "ca","nu","hc") off the ICU
// locale, mapping the BCP-47 key/value through ICU's legacy<->unicode tables.
std::string LocaleUnicodeExtension(const std::string& icu_locale, const char* bcp47_key) {
  const char* legacy_key = uloc_toLegacyKey(bcp47_key);
  if (legacy_key == nullptr) return "";
  char val[64];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_getKeywordValue(icu_locale.c_str(), legacy_key, val, sizeof(val), &status);
  if (U_FAILURE(status) || len <= 0) return "";
  const char* unicode = uloc_toUnicodeLocaleType(bcp47_key, val);
  return unicode != nullptr ? std::string(unicode) : std::string(val, static_cast<size_t>(len));
}

napi_value LocaleExtensionGetter(napi_env env, napi_callback_info info, const char* key) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  if (loc == nullptr) return Undefined(env);
  const std::string value = LocaleUnicodeExtension(*loc, key);
  return value.empty() ? Undefined(env) : MakeString(env, value);
}
napi_value LocaleGetCalendar(napi_env env, napi_callback_info info) { return LocaleExtensionGetter(env, info, "ca"); }
napi_value LocaleGetNumberingSystem(napi_env env, napi_callback_info info) { return LocaleExtensionGetter(env, info, "nu"); }
napi_value LocaleGetHourCycle(napi_env env, napi_callback_info info) { return LocaleExtensionGetter(env, info, "hc"); }
napi_value LocaleGetCollation(napi_env env, napi_callback_info info) { return LocaleExtensionGetter(env, info, "co"); }
napi_value LocaleGetCaseFirst(napi_env env, napi_callback_info info) { return LocaleExtensionGetter(env, info, "kf"); }
napi_value LocaleGetNumeric(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  napi_value out = nullptr;
  napi_get_boolean(env, loc != nullptr && LocaleUnicodeExtension(*loc, "kn") == "true", &out);
  return out;
}

napi_value LocaleGetLanguage(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  return MakeString(env, loc ? LocaleSubtag(*loc, uloc_getLanguage) : "");
}
napi_value LocaleGetScript(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  return MakeString(env, loc ? LocaleSubtag(*loc, uloc_getScript) : "");
}
napi_value LocaleGetRegion(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  return MakeString(env, loc ? LocaleSubtag(*loc, uloc_getCountry) : "");
}
napi_value LocaleGetBaseName(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  if (loc == nullptr) return MakeString(env, "");
  // baseName excludes Unicode extensions: strip the ICU @keywords first.
  char base[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_getBaseName(loc->c_str(), base, sizeof(base), &status);
  if (U_FAILURE(status) || len <= 0) return MakeString(env, IcuLocaleToBcp47(*loc));
  return MakeString(env, IcuLocaleToBcp47(std::string(base, static_cast<size_t>(len))));
}
napi_value LocaleToString(napi_env env, napi_callback_info info) {
  // toString() returns the full tag, including any Unicode extensions.
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  return MakeString(env, loc != nullptr ? IcuLocaleToBcp47(*loc) : "");
}

napi_value NewLocaleFromTag(napi_env env, const std::string& bcp47) {
  napi_value global = nullptr, intl = nullptr, ctor = nullptr, instance = nullptr;
  if (napi_get_global(env, &global) != napi_ok ||
      napi_get_named_property(env, global, "Intl", &intl) != napi_ok ||
      napi_get_named_property(env, intl, "Locale", &ctor) != napi_ok || !IsFunction(env, ctor)) {
    return Undefined(env);
  }
  napi_value arg = MakeString(env, bcp47);
  if (napi_new_instance(env, ctor, 1, &arg, &instance) != napi_ok) return Undefined(env);
  return instance;
}

napi_value LocaleMaximize(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  if (loc == nullptr) return Undefined(env);
  char buf[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_addLikelySubtags(loc->c_str(), buf, sizeof(buf), &status);
  if (U_FAILURE(status) || len <= 0) return NewLocaleFromTag(env, IcuLocaleToBcp47(*loc));
  return NewLocaleFromTag(env, IcuLocaleToBcp47(std::string(buf, len)));
}

napi_value LocaleMinimize(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  std::string* loc = UnwrapLocale(env, self);
  if (loc == nullptr) return Undefined(env);
  char buf[ULOC_FULLNAME_CAPACITY];
  UErrorCode status = U_ZERO_ERROR;
  int32_t len = uloc_minimizeSubtags(loc->c_str(), buf, sizeof(buf), &status);
  if (U_FAILURE(status) || len <= 0) return NewLocaleFromTag(env, IcuLocaleToBcp47(*loc));
  return NewLocaleFromTag(env, IcuLocaleToBcp47(std::string(buf, len)));
}

bool InstallLocale(napi_env env, napi_value intl, std::string* error_out) {
  if (IntlHasUsable(env, intl, "Locale")) return true;
  napi_property_descriptor props[] = {
      {"language", nullptr, nullptr, LocaleGetLanguage, nullptr, nullptr, napi_default, nullptr},
      {"script", nullptr, nullptr, LocaleGetScript, nullptr, nullptr, napi_default, nullptr},
      {"region", nullptr, nullptr, LocaleGetRegion, nullptr, nullptr, napi_default, nullptr},
      {"baseName", nullptr, nullptr, LocaleGetBaseName, nullptr, nullptr, napi_default, nullptr},
      {"calendar", nullptr, nullptr, LocaleGetCalendar, nullptr, nullptr, napi_default, nullptr},
      {"numberingSystem", nullptr, nullptr, LocaleGetNumberingSystem, nullptr, nullptr, napi_default, nullptr},
      {"hourCycle", nullptr, nullptr, LocaleGetHourCycle, nullptr, nullptr, napi_default, nullptr},
      {"collation", nullptr, nullptr, LocaleGetCollation, nullptr, nullptr, napi_default, nullptr},
      {"caseFirst", nullptr, nullptr, LocaleGetCaseFirst, nullptr, nullptr, napi_default, nullptr},
      {"numeric", nullptr, nullptr, LocaleGetNumeric, nullptr, nullptr, napi_default, nullptr},
      {"toString", nullptr, LocaleToString, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"maximize", nullptr, LocaleMaximize, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"minimize", nullptr, LocaleMinimize, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value ctor = nullptr;
  napi_status status = napi_define_class(env, "Locale", NAPI_AUTO_LENGTH, LocaleConstructor, nullptr,
                                         sizeof(props) / sizeof(props[0]), props, &ctor);
  if (status != napi_ok || ctor == nullptr) {
    SetError(error_out, "Failed to define Intl.Locale");
    return false;
  }
  napi_value prototype = nullptr;
  if (napi_get_named_property(env, ctor, "prototype", &prototype) == napi_ok && prototype != nullptr) {
    InstallToStringTag(env, prototype, "Intl.Locale");
  }
  if (napi_set_named_property(env, intl, "Locale", ctor) != napi_ok) {
    SetError(error_out, "Failed to install Intl.Locale");
    return false;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Intl.getCanonicalLocales(locales)  (uloc_forLanguageTag + uloc_toLanguageTag)
// ---------------------------------------------------------------------------

napi_value GetCanonicalLocales(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  std::vector<std::string> tags;
  if (argc > 0 && argv[0] != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, argv[0], &is_array) == napi_ok && is_array) {
      uint32_t len = 0;
      napi_get_array_length(env, argv[0], &len);
      for (uint32_t i = 0; i < len; i++) {
        napi_value el = nullptr;
        if (napi_get_element(env, argv[0], i, &el) == napi_ok) tags.push_back(ValueToString(env, el, ""));
      }
    } else {
      napi_valuetype t = napi_undefined;
      if (napi_typeof(env, argv[0], &t) == napi_ok && t != napi_undefined && t != napi_null) {
        tags.push_back(ValueToString(env, argv[0], ""));
      }
    }
  }

  napi_value out = nullptr;
  napi_create_array(env, &out);
  uint32_t idx = 0;
  for (const std::string& tag : tags) {
    char buf[ULOC_FULLNAME_CAPACITY];
    UErrorCode status = U_ZERO_ERROR;
    int32_t len = uloc_forLanguageTag(tag.c_str(), buf, sizeof(buf), nullptr, &status);
    if (U_FAILURE(status) || len <= 0) {
      return ThrowRange(env, std::string("Invalid language tag: ") + tag);
    }
    napi_set_element(env, out, idx++, MakeString(env, IcuLocaleToBcp47(std::string(buf, len))));
  }
  return out;
}

bool InstallGetCanonicalLocales(napi_env env, napi_value intl) {
  if (IntlHasUsable(env, intl, "getCanonicalLocales")) return true;
  return DefineMethod(env, intl, "getCanonicalLocales", GetCanonicalLocales);
}

// ---------------------------------------------------------------------------
// Intl.Segmenter  (ubrk_*)
// ---------------------------------------------------------------------------

struct SegmenterState {
  std::string locale;
  std::string granularity;  // "grapheme" | "word" | "sentence"
};

void SegmenterFinalize(napi_env, void* data, void*) { delete static_cast<SegmenterState*>(data); }

napi_value SegmenterConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_value new_target = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) return ThrowType(env, "Constructor Intl.Segmenter requires 'new'");

  auto* s = new SegmenterState();
  s->locale = argc > 0 ? ResolveIcuLocale(env, argv[0]) : "en_US";
  s->granularity = GetStringOption(env, argc > 1 ? argv[1] : nullptr, "granularity",
                                   {"grapheme", "word", "sentence"}, "grapheme");
  if (s->granularity.empty()) { delete s; return ThrowRange(env, "Invalid Intl.Segmenter granularity"); }
  if (napi_wrap(env, this_arg, s, SegmenterFinalize, nullptr, nullptr) != napi_ok) {
    delete s;
    return nullptr;
  }
  return this_arg;
}

// Reads `this._parts` (the precomputed array of segment-data objects) stashed on
// a Segments object by SegmenterSegment.
napi_value SegmentsParts(napi_env env, napi_value self) {
  napi_value parts = nullptr;
  napi_get_named_property(env, self, "_parts", &parts);
  return parts;
}

// Segments[Symbol.iterator]() -> delegates to the backing array's iterator.
napi_value SegmentsIterator(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &self, nullptr);
  napi_value parts = SegmentsParts(env, self);
  if (parts == nullptr) return Undefined(env);
  napi_value global = nullptr, symbol = nullptr, key = nullptr, iter_fn = nullptr, iter = nullptr;
  napi_get_global(env, &global);
  napi_get_named_property(env, global, "Symbol", &symbol);
  napi_get_named_property(env, symbol, "iterator", &key);
  if (napi_get_property(env, parts, key, &iter_fn) != napi_ok || !IsFunction(env, iter_fn)) {
    return Undefined(env);
  }
  napi_call_function(env, parts, iter_fn, 0, nullptr, &iter);
  return iter;
}

// Segments.containing(index) -> the segment-data object covering that code-unit index.
napi_value SegmentsContaining(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok) return nullptr;
  int32_t pos = 0;
  if (argc > 0 && argv[0] != nullptr) {
    napi_value num = argv[0];
    napi_valuetype t = napi_undefined;
    napi_typeof(env, argv[0], &t);
    if (t != napi_number) napi_coerce_to_number(env, argv[0], &num);
    double d = 0;
    napi_get_value_double(env, num, &d);
    pos = static_cast<int32_t>(d);
  }
  napi_value parts = SegmentsParts(env, self);
  uint32_t len = 0;
  if (parts == nullptr || napi_get_array_length(env, parts, &len) != napi_ok) return Undefined(env);
  for (uint32_t i = 0; i < len; i++) {
    napi_value part = nullptr;
    if (napi_get_element(env, parts, i, &part) != napi_ok) continue;
    napi_value idx_v = nullptr, seg_v = nullptr;
    int32_t idx = 0;
    napi_get_named_property(env, part, "index", &idx_v);
    napi_get_value_int32(env, idx_v, &idx);
    napi_get_named_property(env, part, "segment", &seg_v);
    size_t seg_units = 0;
    napi_get_value_string_utf16(env, seg_v, nullptr, 0, &seg_units);
    if (pos >= idx && pos < idx + static_cast<int32_t>(seg_units)) return part;
  }
  return Undefined(env);
}

napi_value SegmenterSegment(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) {
    return ThrowType(env, "Intl.Segmenter.prototype.segment called on incompatible receiver");
  }
  auto* s = static_cast<SegmenterState*>(data);
  napi_value input = argv[0] != nullptr ? argv[0] : MakeString(env, "");
  const std::string utf8 = ValueToString(env, input, "");
  const std::u16string text = ToUChars(utf8);

  UBreakIteratorType kind = UBRK_CHARACTER;
  if (s->granularity == "word") kind = UBRK_WORD;
  else if (s->granularity == "sentence") kind = UBRK_SENTENCE;

  UErrorCode status = U_ZERO_ERROR;
  UBreakIterator* bi = ubrk_open(kind, s->locale.c_str(),
                                 reinterpret_cast<const UChar*>(text.data()),
                                 static_cast<int32_t>(text.size()), &status);
  napi_value parts = nullptr;
  napi_create_array(env, &parts);
  uint32_t out_idx = 0;
  const bool is_word = (kind == UBRK_WORD);
  if (U_SUCCESS(status) && bi != nullptr) {
    int32_t start = ubrk_first(bi);
    for (int32_t end = ubrk_next(bi); end != UBRK_DONE; start = end, end = ubrk_next(bi)) {
      napi_value seg = nullptr;
      napi_create_object(env, &seg);
      const std::u16string piece = text.substr(static_cast<size_t>(start), static_cast<size_t>(end - start));
      napi_set_named_property(env, seg, "segment",
          MakeString(env, FromUChars(reinterpret_cast<const UChar*>(piece.data()),
                                     static_cast<int32_t>(piece.size()))));
      napi_value index_v = nullptr;
      napi_create_int32(env, start, &index_v);
      napi_set_named_property(env, seg, "index", index_v);
      napi_set_named_property(env, seg, "input", input);
      if (is_word) {
        napi_value wl = nullptr;
        napi_get_boolean(env, ubrk_getRuleStatus(bi) >= UBRK_WORD_NONE_LIMIT, &wl);
        napi_set_named_property(env, seg, "isWordLike", wl);
      }
      napi_set_element(env, parts, out_idx++, seg);
    }
  }
  if (bi != nullptr) ubrk_close(bi);

  // Build the Segments object: iterable (delegates to _parts) + containing().
  napi_value segments = nullptr;
  napi_create_object(env, &segments);
  napi_set_named_property(env, segments, "_parts", parts);
  DefineMethod(env, segments, "containing", SegmentsContaining);
  napi_value global = nullptr, symbol = nullptr, iter_key = nullptr, iter_fn = nullptr;
  napi_get_global(env, &global);
  napi_get_named_property(env, global, "Symbol", &symbol);
  napi_get_named_property(env, symbol, "iterator", &iter_key);
  if (napi_create_function(env, "[Symbol.iterator]", NAPI_AUTO_LENGTH, SegmentsIterator, nullptr,
                           &iter_fn) == napi_ok) {
    napi_set_property(env, segments, iter_key, iter_fn);
  }
  return segments;
}

napi_value SegmenterResolvedOptions(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr);
  void* data = nullptr;
  napi_unwrap(env, this_arg, &data);
  napi_value out = nullptr;
  napi_create_object(env, &out);
  if (data != nullptr) {
    auto* s = static_cast<SegmenterState*>(data);
    napi_set_named_property(env, out, "locale", MakeString(env, IcuLocaleToBcp47(s->locale)));
    napi_set_named_property(env, out, "granularity", MakeString(env, s->granularity));
  }
  return out;
}

bool InstallSegmenter(napi_env env, napi_value intl, std::string* error_out) {
  napi_property_descriptor methods[] = {
      {"segment", nullptr, SegmenterSegment, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"resolvedOptions", nullptr, SegmenterResolvedOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  return InstallConstructor(env, intl, "Segmenter", SegmenterConstructor, methods, 2,
                            "Intl.Segmenter", error_out);
}

// ---------------------------------------------------------------------------
// Install entry point
// ---------------------------------------------------------------------------

napi_value GetOrCreateIntl(napi_env env, napi_value global) {
  napi_value intl = nullptr;
  bool has_intl = false;
  if (napi_has_named_property(env, global, "Intl", &has_intl) == napi_ok && has_intl) {
    napi_get_named_property(env, global, "Intl", &intl);
  }
  napi_valuetype type = napi_undefined;
  if (intl != nullptr && napi_typeof(env, intl, &type) == napi_ok &&
      (type == napi_object || type == napi_function)) {
    return intl;
  }
  if (napi_create_object(env, &intl) != napi_ok) return nullptr;
  if (napi_set_named_property(env, global, "Intl", intl) != napi_ok) return nullptr;
  return intl;
}

}  // namespace

// ---------------------------------------------------------------------------
// String.prototype.toLocaleLowerCase / toLocaleUpperCase (locale-aware)
// ---------------------------------------------------------------------------
// quickjs-ng maps toLocaleLowerCase/UpperCase straight to the non-locale
// casing, so the locale argument is ignored (e.g. 'I'.toLocaleLowerCase('tr')
// yields 'i' instead of the Turkish dotless 'ı'). Back them with ICU
// u_strToLower/u_strToUpper. Only installed when the engine is not already
// locale-aware (i.e. not on the V8 provider), leaving V8's native casing alone.

bool ValueToUtf16(napi_env env, napi_value value, std::u16string* out) {
  napi_value str = value;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  if (type != napi_string && napi_coerce_to_string(env, value, &str) != napi_ok) return false;
  size_t len = 0;
  if (napi_get_value_string_utf16(env, str, nullptr, 0, &len) != napi_ok) return false;
  out->resize(len);
  size_t copied = 0;
  if (napi_get_value_string_utf16(env, str, out->data(), len + 1, &copied) != napi_ok) return false;
  out->resize(copied);
  return true;
}

// Resolve the toLocaleCase `locales` argument (undefined | string | string[])
// to a single ICU locale id. Undefined/empty -> "" (root/language-neutral).
std::string ResolveCasingLocale(napi_env env, napi_value locales_arg) {
  if (locales_arg == nullptr) return std::string();
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, locales_arg, &type) != napi_ok ||
      type == napi_undefined || type == napi_null) {
    return std::string();
  }
  bool is_array = false;
  if (napi_is_array(env, locales_arg, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    napi_get_array_length(env, locales_arg, &length);
    if (length == 0) return std::string();
    napi_value first = nullptr;
    if (napi_get_element(env, locales_arg, 0, &first) != napi_ok) return std::string();
    return ValueToString(env, first);
  }
  return ValueToString(env, locales_arg);
}

napi_value StringToLocaleCase(napi_env env, napi_callback_info info, bool to_upper) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) {
    return Undefined(env);
  }
  std::u16string input;
  if (!ValueToUtf16(env, this_arg, &input)) {
    return ThrowType(env, "String.prototype.toLocale*Case called on incompatible receiver");
  }
  const std::string locale = ResolveCasingLocale(env, argc >= 1 ? argv[0] : nullptr);

  const UChar* src = reinterpret_cast<const UChar*>(input.data());
  const int32_t src_len = static_cast<int32_t>(input.size());
  UErrorCode status = U_ZERO_ERROR;
  const int32_t needed = to_upper
      ? u_strToUpper(nullptr, 0, src, src_len, locale.c_str(), &status)
      : u_strToLower(nullptr, 0, src, src_len, locale.c_str(), &status);
  if (U_FAILURE(status) && status != U_BUFFER_OVERFLOW_ERROR) {
    return ThrowRange(env, std::string("toLocaleCase failed: ") + u_errorName(status));
  }
  std::u16string result(static_cast<size_t>(needed), u'\0');
  status = U_ZERO_ERROR;
  const int32_t written = to_upper
      ? u_strToUpper(reinterpret_cast<UChar*>(result.data()), needed, src, src_len, locale.c_str(), &status)
      : u_strToLower(reinterpret_cast<UChar*>(result.data()), needed, src, src_len, locale.c_str(), &status);
  if (U_FAILURE(status)) {
    return ThrowRange(env, std::string("toLocaleCase failed: ") + u_errorName(status));
  }
  result.resize(static_cast<size_t>(written));
  napi_value out = nullptr;
  napi_create_string_utf16(env, result.data(), result.size(), &out);
  return out;
}

napi_value StringToLocaleLowerCase(napi_env env, napi_callback_info info) {
  return StringToLocaleCase(env, info, false);
}
napi_value StringToLocaleUpperCase(napi_env env, napi_callback_info info) {
  return StringToLocaleCase(env, info, true);
}

// Override String.prototype.toLocale{Lower,Upper}Case with the ICU-backed,
// locale-aware versions. Gated to the QuickJS provider (whose engine ignores
// the locale argument); on the V8 provider this is compiled out so V8's native
// locale casing is left untouched.
bool InstallLocaleCasing(napi_env env) {
#ifdef EDGE_NAPI_QUICKJS
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;
  napi_value string_ctor = GetNamed(env, global, "String");
  if (string_ctor == nullptr) return false;
  napi_value proto = GetNamed(env, string_ctor, "prototype");
  if (proto == nullptr) return false;
  DefineMethod(env, proto, "toLocaleLowerCase", StringToLocaleLowerCase);
  DefineMethod(env, proto, "toLocaleUpperCase", StringToLocaleUpperCase);
#else
  (void)env;
#endif
  return true;
}

// Route Date.prototype.toLocale{,Date,Time}String through Intl.DateTimeFormat
// (quickjs-only). The engine's own versions ignore Intl and format differently
// (e.g. zero-padded '01/01/1970' vs the numeric '1/1/1970' Node produces).
// Replicates ECMA-402 ToDateTimeOptions defaulting so bare/timeZone-only option
// bags default every component to numeric. On V8 this is compiled out.
bool InstallDateLocaleMethods(napi_env env) {
#ifdef EDGE_NAPI_QUICKJS
  static const char* kSource = R"JS((function(){
  'use strict';
  var DTF = Intl.DateTimeFormat;
  var dateComps = ['weekday','year','month','day'];
  var timeComps = ['dayPeriod','hour','minute','second','fractionalSecondDigits'];
  function toDateTimeOptions(options, required, defaults) {
    options = (options === undefined) ? {} : Object(options);
    var opts = Object.assign({}, options);
    var needDefaults = true;
    if (required === 'date' || required === 'any')
      for (var i = 0; i < dateComps.length; i++)
        if (opts[dateComps[i]] !== undefined) needDefaults = false;
    if (required === 'time' || required === 'any')
      for (var j = 0; j < timeComps.length; j++)
        if (opts[timeComps[j]] !== undefined) needDefaults = false;
    if (opts.dateStyle !== undefined || opts.timeStyle !== undefined) needDefaults = false;
    if (needDefaults && (defaults === 'date' || defaults === 'all')) {
      opts.year = 'numeric'; opts.month = 'numeric'; opts.day = 'numeric';
    }
    if (needDefaults && (defaults === 'time' || defaults === 'all')) {
      opts.hour = 'numeric'; opts.minute = 'numeric'; opts.second = 'numeric';
    }
    return opts;
  }
  function def(name, required, defaults) {
    Object.defineProperty(Date.prototype, name, {
      value: function(locales, options) {
        var t = this.getTime();
        if (t !== t) return 'Invalid Date';
        return new DTF(locales, toDateTimeOptions(options, required, defaults)).format(this);
      },
      writable: true, enumerable: false, configurable: true,
    });
  }
  def('toLocaleString', 'any', 'all');
  def('toLocaleDateString', 'date', 'date');
  def('toLocaleTimeString', 'time', 'time');
})();)JS";
  napi_value source = nullptr;
  if (napi_create_string_utf8(env, kSource, NAPI_AUTO_LENGTH, &source) != napi_ok) return false;
  napi_value result = nullptr;
  if (napi_run_script(env, source, &result) != napi_ok) {
    napi_value ex = nullptr;
    napi_get_and_clear_last_exception(env, &ex);
    return false;
  }
#else
  (void)env;
#endif
  return true;
}

bool EdgeInstallIntl(napi_env env, std::string* error_out) {
  if (env == nullptr) return false;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    SetError(error_out, "Failed to fetch global object for Intl");
    return false;
  }
  napi_value intl = GetOrCreateIntl(env, global);
  if (intl == nullptr) {
    SetError(error_out, "Failed to create Intl object");
    return false;
  }

  if (!InstallListFormat(env, intl, error_out)) return false;
  if (!InstallNumberFormat(env, intl, error_out)) return false;
  if (!InstallDateTimeFormat(env, intl, error_out)) return false;
  if (!InstallPluralRules(env, intl, error_out)) return false;
  if (!InstallCollator(env, intl, error_out)) return false;
  if (!InstallRelativeTimeFormat(env, intl, error_out)) return false;
  if (!InstallDisplayNames(env, intl, error_out)) return false;
  if (!InstallLocale(env, intl, error_out)) return false;
  if (!InstallSegmenter(env, intl, error_out)) return false;
  if (!InstallGetCanonicalLocales(env, intl)) return false;

  // Locale-aware String casing (uses the same ICU backend). Non-fatal.
  InstallLocaleCasing(env);
  // Date.prototype.toLocale* delegating to Intl.DateTimeFormat. Non-fatal.
  InstallDateLocaleMethods(env);

  return true;
}
