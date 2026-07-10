#include <string>
#include <vector>

#include "test_env.h"
#include "edge_builtin_bytecode.h"
#include "edge_bytecode_cache.h"

class Test8BuiltinBytecode : public FixtureTestBase {};

namespace {

constexpr const char kTag[] = "v8-test-tag";

std::vector<edge_builtin_bytecode::FileEntry> SampleEntries() {
  std::vector<edge_builtin_bytecode::FileEntry> entries;
  {
    edge_builtin_bytecode::FileEntry entry;
    entry.kind = edge_builtin_bytecode::Kind::kFnLazyBuiltin;
    entry.id = "fs";
    entry.source_hash = 0x1111111111111111ull;
    entry.payload = {0x01, 0x02, 0x03, 0x04};
    entries.push_back(entry);
  }
  {
    edge_builtin_bytecode::FileEntry entry;
    entry.kind = edge_builtin_bytecode::Kind::kFnBootstrapRealm;
    entry.id = "internal/bootstrap/realm";
    entry.source_hash = 0x2222222222222222ull;
    entry.payload = {0xaa};
    entries.push_back(entry);
  }
  {
    edge_builtin_bytecode::FileEntry entry;
    entry.kind = edge_builtin_bytecode::Kind::kWrapperStandard;
    entry.id = "internal/util";
    entry.source_hash = 0x3333333333333333ull;
    entry.payload = {0xde, 0xad, 0xbe, 0xef, 0x00, 0xff};
    entries.push_back(entry);
  }
  return entries;
}

void ExpectEntriesEqual(const std::vector<edge_builtin_bytecode::FileEntry>& actual,
                        const std::vector<edge_builtin_bytecode::FileEntry>& expected) {
  ASSERT_EQ(actual.size(), expected.size());
  for (size_t i = 0; i < expected.size(); ++i) {
    EXPECT_EQ(actual[i].kind, expected[i].kind) << "entry " << i;
    EXPECT_EQ(actual[i].id, expected[i].id) << "entry " << i;
    EXPECT_EQ(actual[i].source_hash, expected[i].source_hash) << "entry " << i;
    EXPECT_EQ(actual[i].payload, expected[i].payload) << "entry " << i;
  }
}

}  // namespace

TEST_F(Test8BuiltinBytecode, EncodeDecodeRoundTrip) {
  const auto entries = SampleEntries();
  const auto encoded = edge_builtin_bytecode::EncodeFile(kTag, entries);

  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  ASSERT_TRUE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
  ExpectEntriesEqual(decoded, entries);
}

TEST_F(Test8BuiltinBytecode, EmptyFileRoundTrip) {
  const auto encoded = edge_builtin_bytecode::EncodeFile(kTag, {});
  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  ASSERT_TRUE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
  EXPECT_TRUE(decoded.empty());
}

TEST_F(Test8BuiltinBytecode, DecodeRejectsBadMagic) {
  auto encoded = edge_builtin_bytecode::EncodeFile(kTag, SampleEntries());
  encoded[0] ^= 0xff;
  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  EXPECT_FALSE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
}

TEST_F(Test8BuiltinBytecode, DecodeRejectsBadFormatVersion) {
  auto encoded = edge_builtin_bytecode::EncodeFile(kTag, SampleEntries());
  encoded[8] ^= 0xff;
  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  EXPECT_FALSE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
}

TEST_F(Test8BuiltinBytecode, DecodeRejectsWrongEngineTag) {
  const auto encoded = edge_builtin_bytecode::EncodeFile(kTag, SampleEntries());
  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  EXPECT_FALSE(
      edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), "v8-other-tag", &decoded));
}

TEST_F(Test8BuiltinBytecode, DecodeRejectsTruncation) {
  const auto encoded = edge_builtin_bytecode::EncodeFile(kTag, SampleEntries());
  std::vector<edge_builtin_bytecode::FileEntry> decoded;

  // Header-only prefix.
  EXPECT_FALSE(edge_builtin_bytecode::DecodeFile(encoded.data(), 16, kTag, &decoded));
  // Last payload byte cut: the final entry overruns the buffer.
  EXPECT_FALSE(
      edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size() - 1, kTag, &decoded));
}

TEST_F(Test8BuiltinBytecode, DecodeRejectsTrailingGarbage) {
  auto encoded = edge_builtin_bytecode::EncodeFile(kTag, SampleEntries());
  encoded.push_back(0x00);
  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  EXPECT_FALSE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
}

TEST_F(Test8BuiltinBytecode, CorruptedPayloadEntryHandling) {
  const auto entries = SampleEntries();
  auto encoded = edge_builtin_bytecode::EncodeFile(kTag, entries);
  // Flip the last byte: it belongs to the final entry's payload. The
  // container only pins structure; payload integrity is the engine's job
  // (V8 CachedData / the QuickJS provider's QJSB payload header), so the
  // entry survives container decoding on both engines and the engine
  // rejects it at deserialize time.
  encoded.back() ^= 0x01;

  std::vector<edge_builtin_bytecode::FileEntry> decoded;
  ASSERT_TRUE(edge_builtin_bytecode::DecodeFile(encoded.data(), encoded.size(), kTag, &decoded));
  ASSERT_EQ(decoded.size(), entries.size());
  EXPECT_EQ(decoded[0].id, "fs");
  EXPECT_EQ(decoded[1].id, "internal/bootstrap/realm");
}

TEST_F(Test8BuiltinBytecode, CacheFilePathUsesExecPathAndEngineSuffix) {
  const std::string path = edge_builtin_bytecode::BuiltinCacheFilePath();
  const std::string expected_suffix =
      std::string(".builtins") + edge_bytecode_cache::SidecarSuffix();
  ASSERT_GT(path.size(), expected_suffix.size());
  EXPECT_EQ(path.substr(path.size() - expected_suffix.size()), expected_suffix);
}
