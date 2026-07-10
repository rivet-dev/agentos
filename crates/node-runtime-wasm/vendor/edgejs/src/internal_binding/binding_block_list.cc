#include "internal_binding/binding_initializers.h"

#include <array>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include <uv.h>

#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

enum class BlockRuleType : uint8_t {
  kAddress,
  kRange,
  kSubnet,
};

enum class AddressOrdering : int8_t {
  kLess = -1,
  kEquivalent = 0,
  kGreater = 1,
  kUnordered = 2,
};

struct SocketAddressWrap {
  napi_ref wrapper_ref = nullptr;
  int32_t family = AF_INET;
  std::array<uint8_t, 16> bytes{};
  uint16_t port = 0;
  uint32_t flowlabel = 0;
};

struct BlockRule {
  BlockRuleType type = BlockRuleType::kAddress;
  int32_t family = AF_INET;
  int32_t end_family = AF_INET;
  std::array<uint8_t, 16> start{};
  std::array<uint8_t, 16> end{};
  uint8_t prefix = 0;
};

constexpr std::array<uint8_t, 12> kIpv4MappedIpv6Prefix = {
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff,
};

struct BlockListWrap {
  napi_ref wrapper_ref = nullptr;
  std::vector<BlockRule> rules;
};

size_t FamilySize(int32_t family) {
  return family == AF_INET ? 4 : 16;
}

const char* FamilyLabel(int32_t family) {
  return family == AF_INET ? "IPv4" : "IPv6";
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

bool ParseIp(int32_t family, const std::string& text, std::array<uint8_t, 16>* out) {
  if (out == nullptr) return false;
  out->fill(0);
  if (family == AF_INET) {
    return uv_inet_pton(AF_INET, text.c_str(), out->data()) == 0;
  }
  if (family == AF_INET6) {
    return uv_inet_pton(AF_INET6, text.c_str(), out->data()) == 0;
  }
  return false;
}

std::string FormatIp(int32_t family, const std::array<uint8_t, 16>& bytes) {
  char out[INET6_ADDRSTRLEN] = {0};
  if (uv_inet_ntop(family, bytes.data(), out, sizeof(out)) != 0) return {};
  return std::string(out);
}

int CompareIp(const std::array<uint8_t, 16>& a, const std::array<uint8_t, 16>& b, int32_t family) {
  const size_t len = FamilySize(family);
  return std::memcmp(a.data(), b.data(), len);
}

bool IsIpv4MappedIpv6(const std::array<uint8_t, 16>& address) {
  return std::memcmp(address.data(), kIpv4MappedIpv6Prefix.data(), kIpv4MappedIpv6Prefix.size()) == 0;
}

AddressOrdering FromCompareResult(int value) {
  if (value < 0) return AddressOrdering::kLess;
  if (value > 0) return AddressOrdering::kGreater;
  return AddressOrdering::kEquivalent;
}

AddressOrdering ReverseOrdering(AddressOrdering ordering) {
  switch (ordering) {
    case AddressOrdering::kLess:
      return AddressOrdering::kGreater;
    case AddressOrdering::kGreater:
      return AddressOrdering::kLess;
    default:
      return ordering;
  }
}

bool PrefixMatches(const uint8_t* address, const uint8_t* network, uint8_t prefix, size_t size) {
  if (address == nullptr || network == nullptr || prefix > size * 8) return false;
  const size_t full_bytes = prefix / 8;
  const uint8_t rem_bits = prefix % 8;
  if (full_bytes > 0 && std::memcmp(address, network, full_bytes) != 0) return false;
  if (rem_bits == 0) return true;
  const uint8_t mask = static_cast<uint8_t>(0xFFu << (8 - rem_bits));
  return (address[full_bytes] & mask) == (network[full_bytes] & mask);
}

bool IsAddressMatch(int32_t left_family,
                    const std::array<uint8_t, 16>& left,
                    int32_t right_family,
                    const std::array<uint8_t, 16>& right) {
  if (left_family == right_family) {
    return CompareIp(left, right, left_family) == 0;
  }

  if (left_family == AF_INET && right_family == AF_INET6) {
    return IsIpv4MappedIpv6(right) &&
           std::memcmp(left.data(), right.data() + kIpv4MappedIpv6Prefix.size(), 4) == 0;
  }

  if (left_family == AF_INET6 && right_family == AF_INET) {
    return IsIpv4MappedIpv6(left) &&
           std::memcmp(left.data() + kIpv4MappedIpv6Prefix.size(), right.data(), 4) == 0;
  }

  return false;
}

AddressOrdering CompareAddresses(int32_t left_family,
                                 const std::array<uint8_t, 16>& left,
                                 int32_t right_family,
                                 const std::array<uint8_t, 16>& right) {
  if (left_family == right_family) {
    return FromCompareResult(CompareIp(left, right, left_family));
  }

  if (left_family == AF_INET && right_family == AF_INET6) {
    if (!IsIpv4MappedIpv6(right)) return AddressOrdering::kUnordered;
    return FromCompareResult(std::memcmp(left.data(), right.data() + kIpv4MappedIpv6Prefix.size(), 4));
  }

  if (left_family == AF_INET6 && right_family == AF_INET) {
    if (!IsIpv4MappedIpv6(left)) return AddressOrdering::kUnordered;
    return ReverseOrdering(
        FromCompareResult(std::memcmp(right.data(), left.data() + kIpv4MappedIpv6Prefix.size(), 4)));
  }

  return AddressOrdering::kUnordered;
}

bool InSubnet(int32_t address_family,
              const std::array<uint8_t, 16>& address,
              int32_t network_family,
              const std::array<uint8_t, 16>& network,
              uint8_t prefix) {
  if (address_family == network_family) {
    return PrefixMatches(address.data(), network.data(), prefix, FamilySize(network_family));
  }

  if (address_family == AF_INET && network_family == AF_INET6) {
    std::array<uint8_t, 16> mapped = {};
    std::memcpy(mapped.data(), kIpv4MappedIpv6Prefix.data(), kIpv4MappedIpv6Prefix.size());
    std::memcpy(mapped.data() + kIpv4MappedIpv6Prefix.size(), address.data(), 4);
    return PrefixMatches(mapped.data(), network.data(), prefix, mapped.size());
  }

  if (address_family == AF_INET6 && network_family == AF_INET) {
    return IsIpv4MappedIpv6(address) &&
           PrefixMatches(address.data() + kIpv4MappedIpv6Prefix.size(), network.data(), prefix, 4);
  }

  return false;
}

void SocketAddressFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<SocketAddressWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->wrapper_ref != nullptr) napi_delete_reference(env, wrap->wrapper_ref);
  delete wrap;
}

void BlockListFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<BlockListWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->wrapper_ref != nullptr) napi_delete_reference(env, wrap->wrapper_ref);
  delete wrap;
}

SocketAddressWrap* UnwrapSocketAddress(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<SocketAddressWrap*>(data);
}

BlockListWrap* UnwrapBlockList(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<BlockListWrap*>(data);
}

napi_value SocketAddressCtor(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr) return nullptr;

  auto* wrap = new SocketAddressWrap();
  std::string address = argc >= 1 ? ValueToUtf8(env, argv[0]) : "";
  int32_t port = 0;
  int32_t family = AF_INET;
  int32_t flowlabel = 0;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &port);
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &family);
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_int32(env, argv[3], &flowlabel);

  if ((family != AF_INET && family != AF_INET6) || !ParseIp(family, address, &wrap->bytes)) {
    delete wrap;
    napi_throw_range_error(env, nullptr, "Invalid socket address");
    return nullptr;
  }

  wrap->family = family;
  wrap->port = static_cast<uint16_t>(port & 0xFFFF);
  wrap->flowlabel = static_cast<uint32_t>(flowlabel);
  napi_wrap(env, self, wrap, SocketAddressFinalize, nullptr, &wrap->wrapper_ref);
  return self;
}

napi_value SocketAddressDetail(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  SocketAddressWrap* wrap = UnwrapSocketAddress(env, self);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);

  napi_value out = argv[0];
  const std::string address = FormatIp(wrap->family, wrap->bytes);
  SetString(env, out, "address", address.c_str());
  SetInt32(env, out, "port", static_cast<int32_t>(wrap->port));
  SetInt32(env, out, "family", wrap->family);
  napi_value flow_v = nullptr;
  if (napi_create_uint32(env, wrap->flowlabel, &flow_v) == napi_ok && flow_v != nullptr) {
    napi_set_named_property(env, out, "flowlabel", flow_v);
  }
  return out;
}

napi_value SocketAddressFlowLabel(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SocketAddressWrap* wrap = UnwrapSocketAddress(env, self);
  napi_value out = nullptr;
  napi_create_uint32(env, wrap == nullptr ? 0 : wrap->flowlabel, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value BlockListCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok || self == nullptr) return nullptr;
  auto* wrap = new BlockListWrap();
  napi_wrap(env, self, wrap, BlockListFinalize, nullptr, &wrap->wrapper_ref);
  return self;
}

napi_value BlockListAddAddress(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  BlockListWrap* wrap = UnwrapBlockList(env, self);
  SocketAddressWrap* address = argc >= 1 ? UnwrapSocketAddress(env, argv[0]) : nullptr;
  if (wrap == nullptr || address == nullptr) return Undefined(env);

  BlockRule rule;
  rule.type = BlockRuleType::kAddress;
  rule.family = address->family;
  rule.end_family = address->family;
  rule.start = address->bytes;
  rule.end = address->bytes;
  wrap->rules.push_back(rule);
  return Undefined(env);
}

napi_value BlockListAddRange(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  BlockListWrap* wrap = UnwrapBlockList(env, self);
  SocketAddressWrap* start = argc >= 1 ? UnwrapSocketAddress(env, argv[0]) : nullptr;
  SocketAddressWrap* end = argc >= 2 ? UnwrapSocketAddress(env, argv[1]) : nullptr;
  bool ok = false;
  if (wrap != nullptr && start != nullptr && end != nullptr) {
    const AddressOrdering ordering =
        CompareAddresses(start->family, start->bytes, end->family, end->bytes);
    if (ordering != AddressOrdering::kUnordered && ordering != AddressOrdering::kGreater) {
    BlockRule rule;
    rule.type = BlockRuleType::kRange;
    rule.family = start->family;
    rule.end_family = end->family;
    rule.start = start->bytes;
    rule.end = end->bytes;
    wrap->rules.push_back(rule);
    ok = true;
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value BlockListAddSubnet(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  BlockListWrap* wrap = UnwrapBlockList(env, self);
  SocketAddressWrap* network = argc >= 1 ? UnwrapSocketAddress(env, argv[0]) : nullptr;
  int32_t prefix = 0;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &prefix);
  if (wrap == nullptr || network == nullptr) return Undefined(env);

  const int32_t max_prefix = network->family == AF_INET ? 32 : 128;
  if (prefix < 0) prefix = 0;
  if (prefix > max_prefix) prefix = max_prefix;
  BlockRule rule;
  rule.type = BlockRuleType::kSubnet;
  rule.family = network->family;
  rule.end_family = network->family;
  rule.start = network->bytes;
  rule.prefix = static_cast<uint8_t>(prefix);
  wrap->rules.push_back(rule);
  return Undefined(env);
}

napi_value BlockListCheck(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  BlockListWrap* wrap = UnwrapBlockList(env, self);
  SocketAddressWrap* address = argc >= 1 ? UnwrapSocketAddress(env, argv[0]) : nullptr;
  bool blocked = false;
  if (wrap != nullptr && address != nullptr) {
    for (const BlockRule& rule : wrap->rules) {
      switch (rule.type) {
        case BlockRuleType::kAddress:
          if (IsAddressMatch(address->family, address->bytes, rule.family, rule.start)) blocked = true;
          break;
        case BlockRuleType::kRange:
          if (CompareAddresses(address->family, address->bytes, rule.family, rule.start) !=
                  AddressOrdering::kLess &&
              CompareAddresses(address->family, address->bytes, rule.family, rule.start) !=
                  AddressOrdering::kUnordered &&
              CompareAddresses(address->family, address->bytes, rule.end_family, rule.end) !=
                  AddressOrdering::kGreater &&
              CompareAddresses(address->family, address->bytes, rule.end_family, rule.end) !=
                  AddressOrdering::kUnordered) {
            blocked = true;
          }
          break;
        case BlockRuleType::kSubnet:
          if (InSubnet(address->family, address->bytes, rule.family, rule.start, rule.prefix)) blocked = true;
          break;
      }
      if (blocked) break;
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, blocked, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value BlockListGetRules(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  BlockListWrap* wrap = UnwrapBlockList(env, self);
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, wrap == nullptr ? 0 : wrap->rules.size(), &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  if (wrap == nullptr) return out;

  for (uint32_t i = 0; i < wrap->rules.size(); ++i) {
    const BlockRule& rule = wrap->rules[wrap->rules.size() - 1 - i];
    const std::string start = FormatIp(rule.family, rule.start);
    const std::string end = FormatIp(rule.end_family, rule.end);
    std::string text;
    switch (rule.type) {
      case BlockRuleType::kAddress:
        text = std::string("Address: ") + FamilyLabel(rule.family) + " " + start;
        break;
      case BlockRuleType::kRange:
        text = std::string("Range: ") + FamilyLabel(rule.family) + " " + start + "-" + end;
        break;
      case BlockRuleType::kSubnet:
        text = std::string("Subnet: ") + FamilyLabel(rule.family) + " " + start + "/" +
               std::to_string(rule.prefix);
        break;
    }
    napi_value item = nullptr;
    napi_create_string_utf8(env, text.c_str(), NAPI_AUTO_LENGTH, &item);
    if (item != nullptr) napi_set_element(env, out, i, item);
  }
  return out;
}

}  // namespace

napi_value InitBlockList(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  napi_property_descriptor socket_props[] = {
      {"detail", nullptr, SocketAddressDetail, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"flowlabel", nullptr, SocketAddressFlowLabel, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value socket_ctor = nullptr;
  if (napi_define_class(env,
                        "SocketAddress",
                        NAPI_AUTO_LENGTH,
                        SocketAddressCtor,
                        nullptr,
                        sizeof(socket_props) / sizeof(socket_props[0]),
                        socket_props,
                        &socket_ctor) == napi_ok &&
      socket_ctor != nullptr) {
    napi_set_named_property(env, out, "SocketAddress", socket_ctor);
  }

  napi_property_descriptor block_props[] = {
      {"addAddress", nullptr, BlockListAddAddress, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"addRange", nullptr, BlockListAddRange, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"addSubnet", nullptr, BlockListAddSubnet, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"check", nullptr, BlockListCheck, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getRules", nullptr, BlockListGetRules, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value block_ctor = nullptr;
  if (napi_define_class(env,
                        "BlockList",
                        NAPI_AUTO_LENGTH,
                        BlockListCtor,
                        nullptr,
                        sizeof(block_props) / sizeof(block_props[0]),
                        block_props,
                        &block_ctor) == napi_ok &&
      block_ctor != nullptr) {
    napi_set_named_property(env, out, "BlockList", block_ctor);
  }

  SetInt32(env, out, "AF_INET", AF_INET);
  SetInt32(env, out, "AF_INET6", AF_INET6);
  return out;
}

}  // namespace internal_binding
