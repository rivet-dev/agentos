#ifndef EDGE_BYTECODE_IO_H_
#define EDGE_BYTECODE_IO_H_

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

// Little-endian integer codecs and whole-file read / atomic write shared by the
// bytecode container modules (edge_bytecode_cache, edge_builtin_bytecode).
namespace edge_bytecode_cache::io {

inline void WriteU16(std::vector<uint8_t>* out, size_t offset, uint16_t value) {
  (*out)[offset + 0] = static_cast<uint8_t>(value);
  (*out)[offset + 1] = static_cast<uint8_t>(value >> 8);
}

inline void WriteU32(std::vector<uint8_t>* out, size_t offset, uint32_t value) {
  for (int i = 0; i < 4; ++i) (*out)[offset + i] = static_cast<uint8_t>(value >> (8 * i));
}

inline void WriteU64(std::vector<uint8_t>* out, size_t offset, uint64_t value) {
  for (int i = 0; i < 8; ++i) (*out)[offset + i] = static_cast<uint8_t>(value >> (8 * i));
}

inline uint16_t ReadU16(const uint8_t* data, size_t offset) {
  return static_cast<uint16_t>(static_cast<uint16_t>(data[offset]) |
                               (static_cast<uint16_t>(data[offset + 1]) << 8));
}

inline uint32_t ReadU32(const uint8_t* data, size_t offset) {
  uint32_t value = 0;
  for (int i = 0; i < 4; ++i) value |= static_cast<uint32_t>(data[offset + i]) << (8 * i);
  return value;
}

inline uint64_t ReadU64(const uint8_t* data, size_t offset) {
  uint64_t value = 0;
  for (int i = 0; i < 8; ++i) value |= static_cast<uint64_t>(data[offset + i]) << (8 * i);
  return value;
}

// Reads the whole file into `out` (resized to the exact size) in one sized
// read, so the caller can validate in place. False (and empty `out`) on any
// error; never throws. The caller owns tracing.
inline bool ReadFileFully(const std::string& path, std::vector<uint8_t>* out) {
  out->clear();
  std::FILE* in = std::fopen(path.c_str(), "rb");
  if (in == nullptr) return false;
  if (std::fseek(in, 0, SEEK_END) != 0) {
    std::fclose(in);
    return false;
  }
  const long file_size = std::ftell(in);
  if (file_size <= 0 || std::fseek(in, 0, SEEK_SET) != 0) {
    std::fclose(in);
    return false;
  }
  out->resize(static_cast<size_t>(file_size));
  const size_t read = std::fread(out->data(), 1, out->size(), in);
  std::fclose(in);
  if (read != out->size()) {
    out->clear();
    return false;
  }
  return true;
}

// Atomically replaces `path` with `data` via a pid-tagged temp file + rename.
// False on any error (read-only filesystem, permissions, ...); never throws.
inline bool AtomicWriteFile(const std::string& path, const uint8_t* data, size_t size) {
#if defined(_WIN32)
  const int pid = _getpid();
#else
  const int pid = static_cast<int>(getpid());
#endif
  const std::string tmp_path = path + "." + std::to_string(pid) + ".tmp";

  std::error_code ec;
  {
    std::FILE* out = std::fopen(tmp_path.c_str(), "wb");
    if (out == nullptr) return false;
    const size_t written = (size > 0 && data != nullptr) ? std::fwrite(data, 1, size, out) : 0;
    const bool ok = written == size && std::fflush(out) == 0;
    std::fclose(out);
    if (!ok) {
      std::filesystem::remove(tmp_path, ec);
      return false;
    }
  }

  std::filesystem::rename(tmp_path, path, ec);
  if (ec) {
    // Windows rename does not replace an existing destination.
    std::filesystem::remove(path, ec);
    std::filesystem::rename(tmp_path, path, ec);
    if (ec) {
      std::filesystem::remove(tmp_path, ec);
      return false;
    }
  }
  return true;
}

}  // namespace edge_bytecode_cache::io

#endif  // EDGE_BYTECODE_IO_H_
