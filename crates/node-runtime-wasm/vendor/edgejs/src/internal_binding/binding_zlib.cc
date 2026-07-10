#include "internal_binding/binding_initializers.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "brotli/decode.h"
#include "brotli/encode.h"
#include "internal_binding/helpers.h"
#include "edge_async_wrap.h"
#include "zlib.h"
#include "zstd.h"
#include "zstd_errors.h"

namespace internal_binding {

namespace {

constexpr uint8_t kGzipHeaderId1 = 0x1f;
constexpr uint8_t kGzipHeaderId2 = 0x8b;
uint8_t g_empty_buffer = 0;
constexpr size_t kTrackedAllocationHeader =
    std::max(sizeof(size_t), alignof(std::max_align_t));
constexpr int kZDefaultMemLevel = 8;
constexpr int kZDefaultWindowBits = 15;

enum class NodeZlibMode : int32_t {
  kNone = 0,
  kDeflate = 1,
  kInflate = 2,
  kGzip = 3,
  kGunzip = 4,
  kDeflateRaw = 5,
  kInflateRaw = 6,
  kUnzip = 7,
  kBrotliDecode = 8,
  kBrotliEncode = 9,
  kZstdCompress = 10,
  kZstdDecompress = 11,
};

enum class HandleKind : uintptr_t {
  kZlib = 1,
  kBrotliEncoder = 2,
  kBrotliDecoder = 3,
  kZstdCompress = 4,
  kZstdDecompress = 5,
};

struct CompressionError {
  CompressionError() = default;
  CompressionError(const char* message_in, const char* code_in, int err_in)
      : message(message_in != nullptr ? message_in : ""),
        code(code_in != nullptr ? code_in : ""),
        err(err_in) {}

  bool IsError() const { return !code.empty(); }

  std::string message;
  std::string code;
  int err = 0;
};

struct ByteSpan {
  uint8_t* data = nullptr;
  size_t len = 0;
};

class CompressionContextBase {
 public:
  virtual ~CompressionContextBase() = default;
  virtual void Close() = 0;
  virtual void DoWork() = 0;
  virtual void SetBuffers(const uint8_t* in, uint32_t in_len, uint8_t* out, uint32_t out_len) = 0;
  virtual void SetFlush(uint32_t flush) = 0;
  virtual void GetAfterWriteOffsets(uint32_t* avail_in, uint32_t* avail_out) const = 0;
  virtual CompressionError GetErrorInfo() const = 0;
  virtual CompressionError ResetStream() = 0;
  virtual CompressionError SetParams(int32_t first, int32_t second) {
    (void)first;
    (void)second;
    return {};
  }
};

const char* ZlibStrerror(int err) {
  switch (err) {
    case Z_OK:
      return "Z_OK";
    case Z_STREAM_END:
      return "Z_STREAM_END";
    case Z_NEED_DICT:
      return "Z_NEED_DICT";
    case Z_ERRNO:
      return "Z_ERRNO";
    case Z_STREAM_ERROR:
      return "Z_STREAM_ERROR";
    case Z_DATA_ERROR:
      return "Z_DATA_ERROR";
    case Z_MEM_ERROR:
      return "Z_MEM_ERROR";
    case Z_BUF_ERROR:
      return "Z_BUF_ERROR";
    case Z_VERSION_ERROR:
      return "Z_VERSION_ERROR";
    default:
      return "Z_UNKNOWN_ERROR";
  }
}

const char* ZstdStrerror(ZSTD_ErrorCode err) {
  switch (err) {
    case ZSTD_error_no_error:
      return "ZSTD_error_no_error";
    case ZSTD_error_GENERIC:
      return "ZSTD_error_GENERIC";
    case ZSTD_error_prefix_unknown:
      return "ZSTD_error_prefix_unknown";
    case ZSTD_error_version_unsupported:
      return "ZSTD_error_version_unsupported";
    case ZSTD_error_frameParameter_unsupported:
      return "ZSTD_error_frameParameter_unsupported";
    case ZSTD_error_frameParameter_windowTooLarge:
      return "ZSTD_error_frameParameter_windowTooLarge";
    case ZSTD_error_corruption_detected:
      return "ZSTD_error_corruption_detected";
    case ZSTD_error_checksum_wrong:
      return "ZSTD_error_checksum_wrong";
    case ZSTD_error_literals_headerWrong:
      return "ZSTD_error_literals_headerWrong";
    case ZSTD_error_dictionary_corrupted:
      return "ZSTD_error_dictionary_corrupted";
    case ZSTD_error_dictionary_wrong:
      return "ZSTD_error_dictionary_wrong";
    case ZSTD_error_dictionaryCreation_failed:
      return "ZSTD_error_dictionaryCreation_failed";
    case ZSTD_error_parameter_unsupported:
      return "ZSTD_error_parameter_unsupported";
    case ZSTD_error_parameter_combination_unsupported:
      return "ZSTD_error_parameter_combination_unsupported";
    case ZSTD_error_parameter_outOfBound:
      return "ZSTD_error_parameter_outOfBound";
    case ZSTD_error_tableLog_tooLarge:
      return "ZSTD_error_tableLog_tooLarge";
    case ZSTD_error_maxSymbolValue_tooLarge:
      return "ZSTD_error_maxSymbolValue_tooLarge";
    case ZSTD_error_maxSymbolValue_tooSmall:
      return "ZSTD_error_maxSymbolValue_tooSmall";
    case ZSTD_error_stabilityCondition_notRespected:
      return "ZSTD_error_stabilityCondition_notRespected";
    case ZSTD_error_stage_wrong:
      return "ZSTD_error_stage_wrong";
    case ZSTD_error_init_missing:
      return "ZSTD_error_init_missing";
    case ZSTD_error_memory_allocation:
      return "ZSTD_error_memory_allocation";
    case ZSTD_error_workSpace_tooSmall:
      return "ZSTD_error_workSpace_tooSmall";
    case ZSTD_error_dstSize_tooSmall:
      return "ZSTD_error_dstSize_tooSmall";
    case ZSTD_error_srcSize_wrong:
      return "ZSTD_error_srcSize_wrong";
    case ZSTD_error_dstBuffer_null:
      return "ZSTD_error_dstBuffer_null";
    case ZSTD_error_noForwardProgress_destFull:
      return "ZSTD_error_noForwardProgress_destFull";
    case ZSTD_error_noForwardProgress_inputEmpty:
      return "ZSTD_error_noForwardProgress_inputEmpty";
    default:
      return "ZSTD_error_GENERIC";
  }
}

class ZlibContext final : public CompressionContextBase {
 public:
  explicit ZlibContext(NodeZlibMode mode) : mode_(mode) {
    std::memset(&strm_, 0, sizeof(strm_));
  }

  void Init(int level,
            int window_bits,
            int mem_level,
            int strategy,
            std::vector<unsigned char>&& dictionary,
            alloc_func alloc,
            free_func free,
            void* opaque) {
    if (zlib_init_done_ || mode_ == NodeZlibMode::kNone) {
      Close();
    }

    std::memset(&strm_, 0, sizeof(strm_));
    strm_.zalloc = alloc;
    strm_.zfree = free;
    strm_.opaque = opaque;

    level_ = level;
    window_bits_ = window_bits;
    mem_level_ = mem_level;
    strategy_ = strategy;
    flush_ = Z_NO_FLUSH;
    err_ = Z_OK;
    gzip_id_bytes_read_ = 0;
    zlib_init_done_ = false;
    dictionary_ = std::move(dictionary);

    if (mode_ == NodeZlibMode::kGzip || mode_ == NodeZlibMode::kGunzip) {
      window_bits_ += 16;
    }
    if (mode_ == NodeZlibMode::kUnzip) {
      window_bits_ += 32;
    }
    if (mode_ == NodeZlibMode::kDeflateRaw || mode_ == NodeZlibMode::kInflateRaw) {
      window_bits_ *= -1;
    }
  }

  void Close() override {
    if (!zlib_init_done_) {
      dictionary_.clear();
      err_ = Z_OK;
      gzip_id_bytes_read_ = 0;
      std::memset(&strm_, 0, sizeof(strm_));
      return;
    }

    int status = Z_OK;
    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kGzip:
      case NodeZlibMode::kDeflateRaw:
        status = deflateEnd(&strm_);
        break;
      case NodeZlibMode::kInflate:
      case NodeZlibMode::kGunzip:
      case NodeZlibMode::kInflateRaw:
      case NodeZlibMode::kUnzip:
        status = inflateEnd(&strm_);
        break;
      default:
        break;
    }
    (void)status;
    zlib_init_done_ = false;
    dictionary_.clear();
    err_ = Z_OK;
    gzip_id_bytes_read_ = 0;
    std::memset(&strm_, 0, sizeof(strm_));
  }

  void DoWork() override {
    const bool first_init_call = InitZlib();
    if (first_init_call && err_ != Z_OK) return;

    const Bytef* next_expected_header_byte = nullptr;
    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kGzip:
      case NodeZlibMode::kDeflateRaw:
        err_ = deflate(&strm_, flush_);
        break;
      case NodeZlibMode::kUnzip:
        if (strm_.avail_in > 0) {
          next_expected_header_byte = strm_.next_in;
        }

        switch (gzip_id_bytes_read_) {
          case 0:
            if (next_expected_header_byte == nullptr) break;
            if (*next_expected_header_byte == kGzipHeaderId1) {
              gzip_id_bytes_read_ = 1;
              next_expected_header_byte++;
              if (strm_.avail_in == 1) break;
            } else {
              mode_ = NodeZlibMode::kInflate;
              break;
            }
            [[fallthrough]];
          case 1:
            if (next_expected_header_byte == nullptr) break;
            if (*next_expected_header_byte == kGzipHeaderId2) {
              gzip_id_bytes_read_ = 2;
              mode_ = NodeZlibMode::kGunzip;
            } else {
              mode_ = NodeZlibMode::kInflate;
            }
            break;
          default:
            break;
        }
        [[fallthrough]];
      case NodeZlibMode::kInflate:
      case NodeZlibMode::kGunzip:
      case NodeZlibMode::kInflateRaw:
        err_ = inflate(&strm_, flush_);

        if (mode_ != NodeZlibMode::kInflateRaw &&
            err_ == Z_NEED_DICT &&
            !dictionary_.empty()) {
          err_ = inflateSetDictionary(&strm_, dictionary_.data(), dictionary_.size());
          if (err_ == Z_OK) {
            err_ = inflate(&strm_, flush_);
          } else if (err_ == Z_DATA_ERROR) {
            err_ = Z_NEED_DICT;
          }
        }

        while (strm_.avail_in > 0 &&
               mode_ == NodeZlibMode::kGunzip &&
               err_ == Z_STREAM_END &&
               strm_.next_in[0] != 0x00) {
          ResetStream();
          err_ = inflate(&strm_, flush_);
        }
        break;
      default:
        break;
    }
  }

  void SetBuffers(const uint8_t* in, uint32_t in_len, uint8_t* out, uint32_t out_len) override {
    strm_.avail_in = in_len;
    strm_.next_in = const_cast<Bytef*>(reinterpret_cast<const Bytef*>(in));
    strm_.avail_out = out_len;
    strm_.next_out = reinterpret_cast<Bytef*>(out);
  }

  void SetFlush(uint32_t flush) override {
    flush_ = static_cast<int>(flush);
  }

  void GetAfterWriteOffsets(uint32_t* avail_in, uint32_t* avail_out) const override {
    if (avail_in != nullptr) *avail_in = strm_.avail_in;
    if (avail_out != nullptr) *avail_out = strm_.avail_out;
  }

  CompressionError GetErrorInfo() const override {
    switch (err_) {
      case Z_OK:
      case Z_BUF_ERROR:
        if (strm_.avail_out != 0 && flush_ == Z_FINISH) {
          return ErrorForMessage("unexpected end of file");
        }
        [[fallthrough]];
      case Z_STREAM_END:
        return {};
      case Z_NEED_DICT:
        return dictionary_.empty() ? ErrorForMessage("Missing dictionary")
                                   : ErrorForMessage("Bad dictionary");
      default:
        return ErrorForMessage("Zlib error");
    }
  }

  CompressionError ResetStream() override {
    const bool first_init_call = InitZlib();
    if (first_init_call && err_ != Z_OK) {
      return ErrorForMessage("Failed to init stream before reset");
    }

    err_ = Z_OK;
    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kDeflateRaw:
      case NodeZlibMode::kGzip:
        err_ = deflateReset(&strm_);
        break;
      case NodeZlibMode::kInflate:
      case NodeZlibMode::kInflateRaw:
      case NodeZlibMode::kGunzip:
        err_ = inflateReset(&strm_);
        break;
      default:
        break;
    }

    if (err_ != Z_OK) return ErrorForMessage("Failed to reset stream");
    return SetDictionary();
  }

  CompressionError SetParams(int32_t level, int32_t strategy) override {
    const bool first_init_call = InitZlib();
    if (first_init_call && err_ != Z_OK) {
      return ErrorForMessage("Failed to init stream before set parameters");
    }

    err_ = Z_OK;
    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kDeflateRaw:
        err_ = deflateParams(&strm_, level, strategy);
        break;
      default:
        break;
    }

    if (err_ != Z_OK && err_ != Z_BUF_ERROR) {
      return ErrorForMessage("Failed to set parameters");
    }
    return {};
  }

 private:
  CompressionError ErrorForMessage(const char* message) const {
    if (strm_.msg != nullptr) {
      message = strm_.msg;
    }
    return CompressionError(message, ZlibStrerror(err_), err_);
  }

  bool InitZlib() {
    if (zlib_init_done_) return false;

    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kGzip:
      case NodeZlibMode::kDeflateRaw:
        err_ = deflateInit2(
            &strm_, level_, Z_DEFLATED, window_bits_, mem_level_, strategy_);
        break;
      case NodeZlibMode::kInflate:
      case NodeZlibMode::kGunzip:
      case NodeZlibMode::kInflateRaw:
      case NodeZlibMode::kUnzip:
        err_ = inflateInit2(&strm_, window_bits_);
        break;
      default:
        return false;
    }

    if (err_ != Z_OK) {
      dictionary_.clear();
      return true;
    }

    CompressionError dict_error = SetDictionary();
    if (dict_error.IsError()) {
      return true;
    }
    zlib_init_done_ = true;
    return true;
  }

  CompressionError SetDictionary() {
    if (dictionary_.empty()) return {};

    err_ = Z_OK;
    switch (mode_) {
      case NodeZlibMode::kDeflate:
      case NodeZlibMode::kDeflateRaw:
        err_ = deflateSetDictionary(&strm_, dictionary_.data(), dictionary_.size());
        break;
      case NodeZlibMode::kInflateRaw:
        err_ = inflateSetDictionary(&strm_, dictionary_.data(), dictionary_.size());
        break;
      default:
        break;
    }

    if (err_ != Z_OK) return ErrorForMessage("Failed to set dictionary");
    return {};
  }

  bool zlib_init_done_ = false;
  int err_ = Z_OK;
  int flush_ = Z_NO_FLUSH;
  int level_ = Z_DEFAULT_COMPRESSION;
  int mem_level_ = kZDefaultMemLevel;
  NodeZlibMode mode_ = NodeZlibMode::kNone;
  int strategy_ = Z_DEFAULT_STRATEGY;
  int window_bits_ = kZDefaultWindowBits;
  unsigned int gzip_id_bytes_read_ = 0;
  std::vector<unsigned char> dictionary_;
  z_stream strm_{};
};

class BrotliContextBase : public CompressionContextBase {
 public:
  explicit BrotliContextBase(NodeZlibMode mode) : mode_(mode) {}

  void SetBuffers(const uint8_t* in, uint32_t in_len, uint8_t* out, uint32_t out_len) override {
    next_in_ = in;
    next_out_ = out;
    avail_in_ = in_len;
    avail_out_ = out_len;
    error_ = BROTLI_DECODER_NO_ERROR;
    error_string_.clear();
  }

  void SetFlush(uint32_t flush) override {
    flush_ = static_cast<BrotliEncoderOperation>(flush);
  }

  void GetAfterWriteOffsets(uint32_t* avail_in, uint32_t* avail_out) const override {
    if (avail_in != nullptr) *avail_in = static_cast<uint32_t>(avail_in_);
    if (avail_out != nullptr) *avail_out = static_cast<uint32_t>(avail_out_);
  }

 protected:
  NodeZlibMode mode_ = NodeZlibMode::kNone;
  const uint8_t* next_in_ = nullptr;
  uint8_t* next_out_ = nullptr;
  size_t avail_in_ = 0;
  size_t avail_out_ = 0;
  BrotliEncoderOperation flush_ = BROTLI_OPERATION_PROCESS;
  brotli_alloc_func alloc_ = nullptr;
  brotli_free_func free_ = nullptr;
  void* alloc_opaque_ = nullptr;
  BrotliDecoderErrorCode error_ = BROTLI_DECODER_NO_ERROR;
  std::string error_string_;
};

class BrotliEncoderContext final : public BrotliContextBase {
 public:
  explicit BrotliEncoderContext(NodeZlibMode mode) : BrotliContextBase(mode) {}

  CompressionError Init(brotli_alloc_func alloc, brotli_free_func free, void* opaque) {
    alloc_ = alloc;
    free_ = free;
    alloc_opaque_ = opaque;
    state_.reset(BrotliEncoderCreateInstance(alloc, free, opaque));
    last_result_ = true;
    if (!state_) {
      return CompressionError(
          "Could not initialize Brotli instance", "ERR_ZLIB_INITIALIZATION_FAILED", -1);
    }
    return {};
  }

  void Close() override {
    state_.reset();
  }

  void DoWork() override {
    if (!state_) return;
    const uint8_t* next_in = next_in_;
    last_result_ = BrotliEncoderCompressStream(
        state_.get(), flush_, &avail_in_, &next_in, &avail_out_, &next_out_, nullptr);
    next_in_ += static_cast<size_t>(next_in - next_in_);
  }

  CompressionError GetErrorInfo() const override {
    if (!last_result_) {
      return CompressionError(
          "Compression failed", "ERR_BROTLI_COMPRESSION_FAILED", -1);
    }
    return {};
  }

  CompressionError ResetStream() override {
    return Init(alloc_, free_, alloc_opaque_);
  }

  CompressionError SetParams(int32_t key, int32_t value) override {
    if (!state_ ||
        !BrotliEncoderSetParameter(
            state_.get(), static_cast<BrotliEncoderParameter>(key), value)) {
      return CompressionError(
          "Setting parameter failed", "ERR_BROTLI_PARAM_SET_FAILED", -1);
    }
    return {};
  }

 private:
  struct EncoderDeleter {
    void operator()(BrotliEncoderState* state) const {
      if (state != nullptr) BrotliEncoderDestroyInstance(state);
    }
  };

  bool last_result_ = true;
  std::unique_ptr<BrotliEncoderState, EncoderDeleter> state_;
};

class BrotliDecoderContext final : public BrotliContextBase {
 public:
  explicit BrotliDecoderContext(NodeZlibMode mode) : BrotliContextBase(mode) {}

  CompressionError Init(brotli_alloc_func alloc, brotli_free_func free, void* opaque) {
    alloc_ = alloc;
    free_ = free;
    alloc_opaque_ = opaque;
    state_.reset(BrotliDecoderCreateInstance(alloc, free, opaque));
    last_result_ = BROTLI_DECODER_RESULT_SUCCESS;
    error_ = BROTLI_DECODER_NO_ERROR;
    error_string_.clear();
    if (!state_) {
      return CompressionError(
          "Could not initialize Brotli instance", "ERR_ZLIB_INITIALIZATION_FAILED", -1);
    }
    return {};
  }

  void Close() override {
    state_.reset();
  }

  void DoWork() override {
    if (!state_) return;
    const uint8_t* next_in = next_in_;
    last_result_ = BrotliDecoderDecompressStream(
        state_.get(), &avail_in_, &next_in, &avail_out_, &next_out_, nullptr);
    next_in_ += static_cast<size_t>(next_in - next_in_);
    if (last_result_ == BROTLI_DECODER_RESULT_ERROR) {
      error_ = BrotliDecoderGetErrorCode(state_.get());
      error_string_ = std::string("ERR_") + BrotliDecoderErrorString(error_);
    }
  }

  CompressionError GetErrorInfo() const override {
    if (error_ != BROTLI_DECODER_NO_ERROR) {
      return CompressionError(
          "Decompression failed", error_string_.c_str(), static_cast<int>(error_));
    }
    if (flush_ == BROTLI_OPERATION_FINISH &&
        last_result_ == BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT) {
      return CompressionError("unexpected end of file", "Z_BUF_ERROR", Z_BUF_ERROR);
    }
    return {};
  }

  CompressionError ResetStream() override {
    return Init(alloc_, free_, alloc_opaque_);
  }

  CompressionError SetParams(int32_t key, int32_t value) override {
    if (!state_ ||
        !BrotliDecoderSetParameter(
            state_.get(), static_cast<BrotliDecoderParameter>(key), value)) {
      return CompressionError(
          "Setting parameter failed", "ERR_BROTLI_PARAM_SET_FAILED", -1);
    }
    return {};
  }

 private:
  struct DecoderDeleter {
    void operator()(BrotliDecoderState* state) const {
      if (state != nullptr) BrotliDecoderDestroyInstance(state);
    }
  };

  BrotliDecoderResult last_result_ = BROTLI_DECODER_RESULT_SUCCESS;
  std::unique_ptr<BrotliDecoderState, DecoderDeleter> state_;
};

class ZstdContextBase : public CompressionContextBase {
 public:
  void Close() override {}

  void SetBuffers(const uint8_t* in, uint32_t in_len, uint8_t* out, uint32_t out_len) override {
    input_.src = in;
    input_.size = in_len;
    input_.pos = 0;
    output_.dst = out;
    output_.size = out_len;
    output_.pos = 0;
    error_ = ZSTD_error_no_error;
    error_string_.clear();
    error_code_string_.clear();
  }

  void SetFlush(uint32_t flush) override {
    flush_ = static_cast<ZSTD_EndDirective>(flush);
  }

  void GetAfterWriteOffsets(uint32_t* avail_in, uint32_t* avail_out) const override {
    if (avail_in != nullptr) *avail_in = static_cast<uint32_t>(input_.size - input_.pos);
    if (avail_out != nullptr) *avail_out = static_cast<uint32_t>(output_.size - output_.pos);
  }

  CompressionError GetErrorInfo() const override {
    if (error_ != ZSTD_error_no_error) {
      return CompressionError(
          error_string_.c_str(), error_code_string_.c_str(), static_cast<int>(error_));
    }
    return {};
  }

 protected:
  ZSTD_EndDirective flush_ = ZSTD_e_continue;
  ZSTD_inBuffer input_{nullptr, 0, 0};
  ZSTD_outBuffer output_{nullptr, 0, 0};
  ZSTD_ErrorCode error_ = ZSTD_error_no_error;
  std::string error_string_;
  std::string error_code_string_;
};

class ZstdCompressContext final : public ZstdContextBase {
 public:
  CompressionError Init(uint64_t pledged_src_size, std::string_view dictionary = {}) {
    pledged_src_size_ = pledged_src_size;
    cctx_.reset(ZSTD_createCCtx());
    if (!cctx_) {
      return CompressionError(
          "Could not initialize zstd instance", "ERR_ZLIB_INITIALIZATION_FAILED", -1);
    }

    if (!dictionary.empty()) {
      const size_t ret =
          ZSTD_CCtx_loadDictionary(cctx_.get(), dictionary.data(), dictionary.size());
      if (ZSTD_isError(ret)) {
        return CompressionError(
            "Failed to load zstd dictionary", "ERR_ZLIB_DICTIONARY_LOAD_FAILED", -1);
      }
    }

    const size_t result = ZSTD_CCtx_setPledgedSrcSize(cctx_.get(), pledged_src_size);
    if (ZSTD_isError(result)) {
      return CompressionError(
          "Could not set pledged src size", "ERR_ZLIB_INITIALIZATION_FAILED", -1);
    }
    return {};
  }

  void DoWork() override {
    if (!cctx_) return;
    const size_t remaining =
        ZSTD_compressStream2(cctx_.get(), &output_, &input_, flush_);
    if (ZSTD_isError(remaining)) {
      error_ = ZSTD_getErrorCode(remaining);
      error_code_string_ = ZstdStrerror(error_);
      error_string_ = ZSTD_getErrorString(error_);
    }
  }

  CompressionError ResetStream() override {
    return Init(pledged_src_size_);
  }

  CompressionError SetParams(int32_t key, int32_t value) override {
    const size_t result = ZSTD_CCtx_setParameter(
        cctx_.get(), static_cast<ZSTD_cParameter>(key), value);
    if (ZSTD_isError(result)) {
      return CompressionError(
          "Setting parameter failed", "ERR_ZSTD_PARAM_SET_FAILED", -1);
    }
    return {};
  }

 private:
  struct CompressDeleter {
    void operator()(ZSTD_CCtx* cctx) const {
      if (cctx != nullptr) ZSTD_freeCCtx(cctx);
    }
  };

  std::unique_ptr<ZSTD_CCtx, CompressDeleter> cctx_;
  uint64_t pledged_src_size_ = ZSTD_CONTENTSIZE_UNKNOWN;
};

class ZstdDecompressContext final : public ZstdContextBase {
 public:
  CompressionError Init(uint64_t /*pledged_src_size*/, std::string_view dictionary = {}) {
    dctx_.reset(ZSTD_createDCtx());
    if (!dctx_) {
      return CompressionError(
          "Could not initialize zstd instance", "ERR_ZLIB_INITIALIZATION_FAILED", -1);
    }

    if (!dictionary.empty()) {
      const size_t ret =
          ZSTD_DCtx_loadDictionary(dctx_.get(), dictionary.data(), dictionary.size());
      if (ZSTD_isError(ret)) {
        return CompressionError(
            "Failed to load zstd dictionary", "ERR_ZLIB_DICTIONARY_LOAD_FAILED", -1);
      }
    }
    return {};
  }

  void DoWork() override {
    if (!dctx_) return;
    const size_t ret = ZSTD_decompressStream(dctx_.get(), &output_, &input_);
    if (ZSTD_isError(ret)) {
      error_ = ZSTD_getErrorCode(ret);
      error_code_string_ = ZstdStrerror(error_);
      error_string_ = ZSTD_getErrorString(error_);
    }
  }

  CompressionError ResetStream() override {
    return Init(ZSTD_CONTENTSIZE_UNKNOWN);
  }

  CompressionError SetParams(int32_t key, int32_t value) override {
    const size_t result = ZSTD_DCtx_setParameter(
        dctx_.get(), static_cast<ZSTD_dParameter>(key), value);
    if (ZSTD_isError(result)) {
      return CompressionError(
          "Setting parameter failed", "ERR_ZSTD_PARAM_SET_FAILED", -1);
    }
    return {};
  }

 private:
  struct DecompressDeleter {
    void operator()(ZSTD_DCtx* dctx) const {
      if (dctx != nullptr) ZSTD_freeDCtx(dctx);
    }
  };

  std::unique_ptr<ZSTD_DCtx, DecompressDeleter> dctx_;
};

struct CompressionHandle {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref process_callback_ref = nullptr;
  napi_ref write_result_ref = nullptr;
  napi_async_work async_work = nullptr;
  std::unique_ptr<CompressionContextBase> context;
  HandleKind kind = HandleKind::kZlib;
  NodeZlibMode mode = NodeZlibMode::kNone;
  std::atomic<int64_t> pending_external_memory{0};
  int64_t reported_external_memory = 0;
  int64_t async_id = -1;
  int32_t provider_type = kEdgeProviderZlib;
  bool init_done = false;
  bool write_in_progress = false;
  bool pending_close = false;
  bool closed = false;
};

size_t TypedArrayElementSize(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
    case napi_float16_array:
      return 2;
    case napi_int32_array:
    case napi_uint32_array:
    case napi_float32_array:
      return 4;
    case napi_float64_array:
    case napi_bigint64_array:
    case napi_biguint64_array:
      return 8;
    default:
      return 1;
  }
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok &&
         (type == napi_undefined || type == napi_null);
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok) return nullptr;
  return value;
}

void ClearPendingException(napi_env env) {
  bool pending = false;
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value ignored = nullptr;
    napi_get_and_clear_last_exception(env, &ignored);
  }
}

bool CoerceToUint32(napi_env env, napi_value value, uint32_t* out) {
  if (env == nullptr || value == nullptr || out == nullptr) return false;
  napi_value number = nullptr;
  return napi_coerce_to_number(env, value, &number) == napi_ok &&
         number != nullptr &&
         napi_get_value_uint32(env, number, out) == napi_ok;
}

bool CoerceToInt32(napi_env env, napi_value value, int32_t* out) {
  if (env == nullptr || value == nullptr || out == nullptr) return false;
  napi_value number = nullptr;
  return napi_coerce_to_number(env, value, &number) == napi_ok &&
         number != nullptr &&
         napi_get_value_int32(env, number, out) == napi_ok;
}

bool ExtractByteSpan(napi_env env, napi_value value, ByteSpan* out) {
  if (out == nullptr) return false;
  out->data = nullptr;
  out->len = 0;
  if (env == nullptr || value == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, value, &raw, &len) == napi_ok) {
      out->data = (raw != nullptr) ? static_cast<uint8_t*>(raw) : &g_empty_buffer;
      out->len = len;
      return true;
    }
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t len = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(
            env, value, &type, &len, &raw, &arraybuffer, &byte_offset) == napi_ok &&
        (raw != nullptr || len == 0)) {
      out->data = (raw != nullptr) ? static_cast<uint8_t*>(raw) : &g_empty_buffer;
      out->len = len * TypedArrayElementSize(type);
      return true;
    }
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t len = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, value, &len, &raw, &arraybuffer, &byte_offset) ==
            napi_ok &&
        (raw != nullptr || len == 0)) {
      out->data = (raw != nullptr) ? static_cast<uint8_t*>(raw) : &g_empty_buffer;
      out->len = len;
      return true;
    }
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t len = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &len) == napi_ok &&
        (raw != nullptr || len == 0)) {
      out->data = (raw != nullptr) ? static_cast<uint8_t*>(raw) : &g_empty_buffer;
      out->len = len;
      return true;
    }
  }

  return false;
}

bool ExtractBinarySequence(napi_env env,
                           napi_value value,
                           const uint8_t** data,
                           size_t* len,
                           std::string* temp_utf8) {
  if (data == nullptr || len == nullptr || temp_utf8 == nullptr) return false;
  *data = nullptr;
  *len = 0;
  temp_utf8->clear();

  ByteSpan span;
  if (ExtractByteSpan(env, value, &span)) {
    *data = span.data;
    *len = span.len;
    return true;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) == napi_ok && type == napi_string) {
    size_t text_len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &text_len) != napi_ok) return false;
    temp_utf8->assign(text_len, '\0');
    size_t written = 0;
    if (napi_get_value_string_utf8(
            env, value, temp_utf8->data(), temp_utf8->size() + 1, &written) != napi_ok) {
      temp_utf8->clear();
      return false;
    }
    temp_utf8->resize(written);
    *data = reinterpret_cast<const uint8_t*>(temp_utf8->data());
    *len = temp_utf8->size();
    return true;
  }

  return false;
}

bool ExtractUint32ArrayData(napi_env env, napi_value value, uint32_t** data, size_t* len) {
  if (data == nullptr || len == nullptr) return false;
  *data = nullptr;
  *len = 0;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) != napi_ok || !is_typedarray) return false;
  napi_typedarray_type type = napi_uint8_array;
  size_t array_len = 0;
  void* raw = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(
          env, value, &type, &array_len, &raw, &arraybuffer, &byte_offset) != napi_ok ||
      type != napi_uint32_array || raw == nullptr) {
    return false;
  }
  *data = static_cast<uint32_t*>(raw);
  *len = array_len;
  return true;
}

bool StoreWriteResultRef(CompressionHandle* handle, napi_value value) {
  if (handle == nullptr || handle->env == nullptr) return false;
  uint32_t* write_result = nullptr;
  size_t write_result_len = 0;
  if (!ExtractUint32ArrayData(handle->env, value, &write_result, &write_result_len) || write_result_len < 2) {
    return false;
  }

  DeleteRefIfPresent(handle->env, &handle->write_result_ref);
  return napi_create_reference(handle->env, value, 1, &handle->write_result_ref) == napi_ok &&
         handle->write_result_ref != nullptr;
}

bool GetWriteResultData(CompressionHandle* handle, uint32_t** data_out) {
  if (data_out == nullptr) return false;
  *data_out = nullptr;
  if (handle == nullptr || handle->env == nullptr || handle->write_result_ref == nullptr) return false;

  napi_value value = GetRefValue(handle->env, handle->write_result_ref);
  if (value == nullptr) return false;

  size_t length = 0;
  return ExtractUint32ArrayData(handle->env, value, data_out, &length) && length >= 2;
}

CompressionHandle* UnwrapHandle(napi_env env,
                                napi_callback_info info,
                                napi_value* self = nullptr,
                                size_t argc_capacity = 0,
                                size_t* argc = nullptr,
                                napi_value* argv = nullptr) {
  napi_value this_arg = nullptr;
  size_t local_argc = argc_capacity;
  if (napi_get_cb_info(env, info, &local_argc, argv, &this_arg, nullptr) != napi_ok) {
    return nullptr;
  }
  if (argc != nullptr) *argc = local_argc;
  if (self != nullptr) *self = this_arg;
  CompressionHandle* handle = nullptr;
  if (this_arg == nullptr ||
      napi_unwrap(env, this_arg, reinterpret_cast<void**>(&handle)) != napi_ok) {
    return nullptr;
  }
  return handle;
}

CompressionHandle* AllocateTrackedHandle(void* data) {
  return static_cast<CompressionHandle*>(data);
}

bool AddTrackedAllocation(CompressionHandle* handle, size_t size) {
  if (handle == nullptr) return false;
  handle->pending_external_memory.fetch_add(static_cast<int64_t>(size), std::memory_order_relaxed);
  return true;
}

void SubtractTrackedAllocation(CompressionHandle* handle, size_t size) {
  if (handle == nullptr) return;
  handle->pending_external_memory.fetch_sub(static_cast<int64_t>(size), std::memory_order_relaxed);
}

void* AllocTracked(void* data, size_t size) {
  CompressionHandle* handle = AllocateTrackedHandle(data);
  if (handle == nullptr) return nullptr;
  if (size > std::numeric_limits<size_t>::max() - kTrackedAllocationHeader) {
    return nullptr;
  }
  const size_t total = size + kTrackedAllocationHeader;
  auto* memory = static_cast<char*>(std::malloc(total));
  if (memory == nullptr) return nullptr;
  *reinterpret_cast<size_t*>(memory) = total;
  AddTrackedAllocation(handle, total);
  return memory + kTrackedAllocationHeader;
}

void FreeTracked(void* data, void* pointer) {
  if (pointer == nullptr) return;
  CompressionHandle* handle = AllocateTrackedHandle(data);
  auto* real_pointer = static_cast<char*>(pointer) - kTrackedAllocationHeader;
  const size_t total = *reinterpret_cast<size_t*>(real_pointer);
  SubtractTrackedAllocation(handle, total);
  std::free(real_pointer);
}

void* AllocForZlib(void* data, uInt items, uInt size) {
  if (items != 0 && size > std::numeric_limits<size_t>::max() / items) {
    return nullptr;
  }
  return AllocTracked(data, static_cast<size_t>(items) * static_cast<size_t>(size));
}

void FreeForZlib(void* data, void* pointer) {
  FreeTracked(data, pointer);
}

void* AllocForBrotli(void* data, size_t size) {
  return AllocTracked(data, size);
}

void ReportExternalMemory(CompressionHandle* handle) {
  if (handle == nullptr || handle->env == nullptr) return;
  const int64_t delta =
      handle->pending_external_memory.exchange(0, std::memory_order_relaxed);
  if (delta == 0) return;
  int64_t adjusted = 0;
  if (napi_adjust_external_memory(handle->env, delta, &adjusted) == napi_ok) {
    handle->reported_external_memory += delta;
  }
}

napi_value GetWrapperValue(CompressionHandle* handle) {
  if (handle == nullptr) return nullptr;
  return GetRefValue(handle->env, handle->wrapper_ref);
}

void QueueDestroyIfNeeded(CompressionHandle* handle) {
  if (handle == nullptr || handle->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(handle->env, handle->async_id);
  handle->async_id = -1;
}

void PinHandle(CompressionHandle* handle) {
  if (handle == nullptr || handle->wrapper_ref == nullptr) return;
  uint32_t refcount = 0;
  napi_reference_ref(handle->env, handle->wrapper_ref, &refcount);
}

void UnpinHandle(CompressionHandle* handle) {
  if (handle == nullptr || handle->wrapper_ref == nullptr) return;
  uint32_t refcount = 0;
  napi_reference_unref(handle->env, handle->wrapper_ref, &refcount);
}

void CloseHandle(CompressionHandle* handle) {
  if (handle == nullptr) return;
  if (handle->write_in_progress) {
    handle->pending_close = true;
    return;
  }
  if (handle->closed) return;
  handle->closed = true;
  handle->pending_close = false;
  if (handle->context != nullptr) {
    handle->context->Close();
  }
  DeleteRefIfPresent(handle->env, &handle->write_result_ref);
  ReportExternalMemory(handle);
}

void UpdateWriteResult(CompressionHandle* handle) {
  uint32_t* write_result = nullptr;
  if (handle == nullptr || handle->context == nullptr || !GetWriteResultData(handle, &write_result)) return;
  uint32_t avail_in = 0;
  uint32_t avail_out = 0;
  handle->context->GetAfterWriteOffsets(&avail_in, &avail_out);
  write_result[0] = avail_out;
  write_result[1] = avail_in;
}

void EmitError(CompressionHandle* handle, const CompressionError& err) {
  if (handle == nullptr || !err.IsError()) return;
  napi_value wrapper = GetWrapperValue(handle);
  if (wrapper == nullptr) return;

  napi_value onerror = nullptr;
  if (napi_get_named_property(handle->env, wrapper, "onerror", &onerror) != napi_ok ||
      !IsFunction(handle->env, onerror)) {
    handle->write_in_progress = false;
    if (handle->pending_close) CloseHandle(handle);
    return;
  }

  napi_value message = nullptr;
  napi_value code = nullptr;
  napi_value errno_value = nullptr;
  napi_create_string_utf8(handle->env, err.message.c_str(), NAPI_AUTO_LENGTH, &message);
  napi_create_string_utf8(handle->env, err.code.c_str(), NAPI_AUTO_LENGTH, &code);
  napi_create_int32(handle->env, err.err, &errno_value);
  napi_value argv[3] = {message, errno_value, code};
  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(
      handle->env, handle->async_id, wrapper, wrapper, onerror, 3, argv, &ignored, 0);

  handle->write_in_progress = false;
  if (handle->pending_close) CloseHandle(handle);
}

void InvokeProcessCallback(CompressionHandle* handle) {
  if (handle == nullptr) return;
  napi_value wrapper = GetWrapperValue(handle);
  napi_value callback = GetRefValue(handle->env, handle->process_callback_ref);
  if (wrapper == nullptr || callback == nullptr || !IsFunction(handle->env, callback)) return;

  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(
      handle->env, handle->async_id, wrapper, wrapper, callback, 0, nullptr, &ignored, 0);
}

void ExecuteCompressionWork(napi_env /*env*/, void* data) {
  auto* handle = static_cast<CompressionHandle*>(data);
  if (handle == nullptr || handle->context == nullptr) return;
  handle->context->DoWork();
}

void CompleteCompressionWork(napi_env env, napi_status status, void* data) {
  auto* handle = static_cast<CompressionHandle*>(data);
  if (handle == nullptr) return;

  napi_async_work work = handle->async_work;
  handle->async_work = nullptr;
  if (work != nullptr) {
    napi_delete_async_work(env, work);
  }

  handle->write_in_progress = false;
  ReportExternalMemory(handle);

  if (status == napi_cancelled) {
    CloseHandle(handle);
    UnpinHandle(handle);
    return;
  }

  if (handle->context != nullptr) {
    const CompressionError err = handle->context->GetErrorInfo();
    if (err.IsError()) {
      EmitError(handle, err);
      UnpinHandle(handle);
      return;
    }
  }

  UpdateWriteResult(handle);
  InvokeProcessCallback(handle);

  if (handle->pending_close) {
    CloseHandle(handle);
  }
  UnpinHandle(handle);
}

bool StartAsyncWork(CompressionHandle* handle) {
  if (handle == nullptr || handle->env == nullptr) return false;
  napi_value resource_name = nullptr;
  if (napi_create_string_utf8(handle->env, "zlib", NAPI_AUTO_LENGTH, &resource_name) != napi_ok ||
      resource_name == nullptr) {
    return false;
  }

  PinHandle(handle);
  if (napi_create_async_work(handle->env,
                             nullptr,
                             resource_name,
                             ExecuteCompressionWork,
                             CompleteCompressionWork,
                             handle,
                             &handle->async_work) != napi_ok ||
      handle->async_work == nullptr) {
    UnpinHandle(handle);
    handle->async_work = nullptr;
    return false;
  }

  if (napi_queue_async_work(handle->env, handle->async_work) != napi_ok) {
    napi_delete_async_work(handle->env, handle->async_work);
    handle->async_work = nullptr;
    UnpinHandle(handle);
    return false;
  }
  return true;
}

std::unique_ptr<CompressionContextBase> CreateContext(HandleKind kind, NodeZlibMode mode) {
  switch (kind) {
    case HandleKind::kZlib:
      return std::make_unique<ZlibContext>(mode);
    case HandleKind::kBrotliEncoder:
      return std::make_unique<BrotliEncoderContext>(mode);
    case HandleKind::kBrotliDecoder:
      return std::make_unique<BrotliDecoderContext>(mode);
    case HandleKind::kZstdCompress:
      return std::make_unique<ZstdCompressContext>();
    case HandleKind::kZstdDecompress:
      return std::make_unique<ZstdDecompressContext>();
  }
  return {};
}

void ThrowInitializationError(napi_env env, const char* message) {
  napi_throw_error(env, "ERR_ZLIB_INITIALIZATION_FAILED", message);
}

void ThrowInvalidArgValue(napi_env env, const char* message) {
  napi_throw_range_error(env, "ERR_INVALID_ARG_VALUE", message);
}

void CompressionFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* handle = static_cast<CompressionHandle*>(data);
  if (handle == nullptr) return;
  handle->env = env;
  CloseHandle(handle);
  ReportExternalMemory(handle);
  QueueDestroyIfNeeded(handle);
  DeleteRefIfPresent(env, &handle->write_result_ref);
  DeleteRefIfPresent(env, &handle->process_callback_ref);
  DeleteRefIfPresent(env, &handle->wrapper_ref);
  delete handle;
}

napi_value CompressionCtor(napi_env env,
                           napi_callback_info info,
                           HandleKind kind,
                           NodeZlibMode fallback_mode) {
  napi_value self = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr) {
    return Undefined(env);
  }

  NodeZlibMode mode = fallback_mode;
  if (argc >= 1 && argv[0] != nullptr && kind != HandleKind::kZstdCompress &&
      kind != HandleKind::kZstdDecompress) {
    int32_t raw_mode = static_cast<int32_t>(fallback_mode);
    if (CoerceToInt32(env, argv[0], &raw_mode)) {
      mode = static_cast<NodeZlibMode>(raw_mode);
    }
  }

  auto* handle = new CompressionHandle();
  handle->env = env;
  handle->kind = kind;
  handle->mode = mode;
  handle->context = CreateContext(kind, mode);
  handle->async_id = EdgeAsyncWrapNextId(env);

  if (napi_wrap(env, self, handle, CompressionFinalize, nullptr, &handle->wrapper_ref) != napi_ok) {
    delete handle;
    return Undefined(env);
  }

  EdgeAsyncWrapEmitInit(env,
                       handle->async_id,
                       handle->provider_type,
                       EdgeAsyncWrapExecutionAsyncId(env),
                       self);
  return self;
}

napi_value ZlibCtor(napi_env env, napi_callback_info info) {
  return CompressionCtor(env, info, HandleKind::kZlib, NodeZlibMode::kNone);
}

napi_value BrotliEncoderCtor(napi_env env, napi_callback_info info) {
  return CompressionCtor(env, info, HandleKind::kBrotliEncoder, NodeZlibMode::kBrotliEncode);
}

napi_value BrotliDecoderCtor(napi_env env, napi_callback_info info) {
  return CompressionCtor(env, info, HandleKind::kBrotliDecoder, NodeZlibMode::kBrotliDecode);
}

napi_value ZstdCompressCtor(napi_env env, napi_callback_info info) {
  return CompressionCtor(env, info, HandleKind::kZstdCompress, NodeZlibMode::kZstdCompress);
}

napi_value ZstdDecompressCtor(napi_env env, napi_callback_info info) {
  return CompressionCtor(env, info, HandleKind::kZstdDecompress, NodeZlibMode::kZstdDecompress);
}

void StoreProcessCallback(CompressionHandle* handle, napi_value callback) {
  if (handle == nullptr) return;
  DeleteRefIfPresent(handle->env, &handle->process_callback_ref);
  if (callback != nullptr && IsFunction(handle->env, callback)) {
    napi_create_reference(handle->env, callback, 1, &handle->process_callback_ref);
  }
}

napi_value CompressionInit(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  napi_value argv[7] = {nullptr};
  size_t argc = 7;
  CompressionHandle* handle = UnwrapHandle(env, info, &self, 7, &argc, argv);
  if (handle == nullptr || handle->context == nullptr) return Undefined(env);

  handle->closed = false;
  handle->pending_close = false;
  handle->init_done = true;

  switch (handle->kind) {
    case HandleKind::kZlib: {
      if (argc == 5) {
        std::fputs(
            "WARNING: You are likely using a version of node-tar or npm that "
            "is incompatible with this version of Node.js.\nPlease use either "
            "the version of npm that is bundled with Node.js, or a version of "
            "npm (> 5.5.1 or < 5.4.0) or node-tar (> 4.0.1) that is "
            "compatible with Node.js 9 and above.\n",
            stderr);
      }
      if (argc != 7) {
        std::abort();
      }
      int32_t window_bits = kZDefaultWindowBits;
      int32_t level = Z_DEFAULT_COMPRESSION;
      int32_t mem_level = kZDefaultMemLevel;
      int32_t strategy = Z_DEFAULT_STRATEGY;
      CoerceToInt32(env, argv[0], &window_bits);
      CoerceToInt32(env, argv[1], &level);
      CoerceToInt32(env, argv[2], &mem_level);
      CoerceToInt32(env, argv[3], &strategy);

      uint32_t* write_result = nullptr;
      size_t write_result_len = 0;
      if (!ExtractUint32ArrayData(env, argv[4], &write_result, &write_result_len) ||
          write_result_len < 2) {
        return Undefined(env);
      }
      if (!StoreWriteResultRef(handle, argv[4])) return Undefined(env);
      StoreProcessCallback(handle, argv[5]);

      std::vector<unsigned char> dictionary;
      if (argc >= 7 && !IsNullOrUndefined(env, argv[6])) {
        ByteSpan span;
        if (ExtractByteSpan(env, argv[6], &span) && span.data != nullptr && span.len > 0) {
          dictionary.assign(span.data, span.data + span.len);
        }
      }

      auto* zlib_context = dynamic_cast<ZlibContext*>(handle->context.get());
      if (zlib_context != nullptr) {
        zlib_context->Init(level,
                           window_bits,
                           mem_level,
                           strategy,
                           std::move(dictionary),
                           AllocForZlib,
                           FreeForZlib,
                           handle);
      }
      ReportExternalMemory(handle);
      break;
    }
    case HandleKind::kBrotliEncoder:
    case HandleKind::kBrotliDecoder: {
      if (argc < 3) return Undefined(env);
      uint32_t* init_params = nullptr;
      size_t init_params_len = 0;
      if (!ExtractUint32ArrayData(env, argv[0], &init_params, &init_params_len) ||
          !StoreWriteResultRef(handle, argv[1])) {
        return Undefined(env);
      }
      StoreProcessCallback(handle, argv[2]);

      CompressionError init_error;
      if (auto* encoder = dynamic_cast<BrotliEncoderContext*>(handle->context.get())) {
        init_error = encoder->Init(AllocForBrotli, FreeForZlib, handle);
      } else if (auto* decoder = dynamic_cast<BrotliDecoderContext*>(handle->context.get())) {
        init_error = decoder->Init(AllocForBrotli, FreeForZlib, handle);
      }
      ReportExternalMemory(handle);

      if (init_error.IsError()) {
        EmitError(handle, init_error);
        ThrowInitializationError(env, "Initialization failed");
        return nullptr;
      }

      for (size_t i = 0; i < init_params_len; ++i) {
        if (init_params[i] == std::numeric_limits<uint32_t>::max()) continue;
        CompressionError err =
            handle->context->SetParams(static_cast<int32_t>(i), static_cast<int32_t>(init_params[i]));
        if (err.IsError()) {
          EmitError(handle, err);
          ThrowInitializationError(env, "Initialization failed");
          return nullptr;
        }
      }
      break;
    }
    case HandleKind::kZstdCompress:
    case HandleKind::kZstdDecompress: {
      if (argc < 4) return Undefined(env);
      uint32_t* init_params = nullptr;
      size_t init_params_len = 0;
      if (!ExtractUint32ArrayData(env, argv[0], &init_params, &init_params_len) ||
          !StoreWriteResultRef(handle, argv[2])) {
        return Undefined(env);
      }
      StoreProcessCallback(handle, argv[3]);

      uint64_t pledged_src_size = ZSTD_CONTENTSIZE_UNKNOWN;
      if (!IsNullOrUndefined(env, argv[1])) {
        double value = 0;
        if (napi_get_value_double(env, argv[1], &value) != napi_ok ||
            !std::isfinite(value) ||
            value < 0 ||
            std::floor(value) != value) {
          ThrowInvalidArgValue(env, "pledgedSrcSize should be a non-negative integer");
          return nullptr;
        }
        pledged_src_size = static_cast<uint64_t>(value);
      }

      std::string_view dictionary;
      ByteSpan dictionary_span;
      if (argc >= 5 && !IsNullOrUndefined(env, argv[4]) &&
          ExtractByteSpan(env, argv[4], &dictionary_span) &&
          dictionary_span.data != nullptr) {
        dictionary = std::string_view(
            reinterpret_cast<const char*>(dictionary_span.data), dictionary_span.len);
      }

      CompressionError init_error;
      if (auto* compress = dynamic_cast<ZstdCompressContext*>(handle->context.get())) {
        init_error = compress->Init(pledged_src_size, dictionary);
      } else if (auto* decompress = dynamic_cast<ZstdDecompressContext*>(handle->context.get())) {
        init_error = decompress->Init(pledged_src_size, dictionary);
      }
      ReportExternalMemory(handle);

      if (init_error.IsError()) {
        EmitError(handle, init_error);
        ThrowInitializationError(env, init_error.message.c_str());
        return nullptr;
      }

      for (size_t i = 0; i < init_params_len; ++i) {
        if (init_params[i] == std::numeric_limits<uint32_t>::max()) continue;
        CompressionError err =
            handle->context->SetParams(static_cast<int32_t>(i), static_cast<int32_t>(init_params[i]));
        if (err.IsError()) {
          EmitError(handle, err);
          ThrowInitializationError(env, err.message.c_str());
          return nullptr;
        }
      }
      break;
    }
  }

  return Undefined(env);
}

napi_value CompressionParams(napi_env env, napi_callback_info info) {
  napi_value argv[2] = {nullptr};
  size_t argc = 2;
  CompressionHandle* handle = UnwrapHandle(env, info, nullptr, 2, &argc, argv);
  if (handle == nullptr || handle->context == nullptr || argc < 2) return Undefined(env);

  int32_t first = 0;
  int32_t second = 0;
  CoerceToInt32(env, argv[0], &first);
  CoerceToInt32(env, argv[1], &second);
  const CompressionError err = handle->context->SetParams(first, second);
  ReportExternalMemory(handle);
  if (err.IsError()) EmitError(handle, err);
  return Undefined(env);
}

napi_value CompressionReset(napi_env env, napi_callback_info info) {
  CompressionHandle* handle = UnwrapHandle(env, info);
  if (handle == nullptr || handle->context == nullptr) return Undefined(env);
  const CompressionError err = handle->context->ResetStream();
  ReportExternalMemory(handle);
  if (err.IsError()) EmitError(handle, err);
  return Undefined(env);
}

napi_value CompressionClose(napi_env env, napi_callback_info info) {
  CompressionHandle* handle = UnwrapHandle(env, info);
  if (handle == nullptr) return Undefined(env);
  CloseHandle(handle);
  return Undefined(env);
}

napi_value CompressionGetAsyncId(napi_env env, napi_callback_info info) {
  CompressionHandle* handle = UnwrapHandle(env, info);
  napi_value out = nullptr;
  napi_create_int64(env, handle != nullptr ? handle->async_id : -1, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CompressionGetProviderType(napi_env env, napi_callback_info info) {
  CompressionHandle* handle = UnwrapHandle(env, info);
  napi_value out = nullptr;
  napi_create_int32(env, handle != nullptr ? handle->provider_type : 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CompressionAsyncReset(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  CompressionHandle* handle = UnwrapHandle(env, info, &self);
  if (handle == nullptr) return Undefined(env);
  EdgeAsyncWrapReset(env, &handle->async_id);
  EdgeAsyncWrapEmitInit(
      env, handle->async_id, handle->provider_type, EdgeAsyncWrapExecutionAsyncId(env), self);
  return Undefined(env);
}

napi_value CompressionWriteCommon(napi_env env, napi_callback_info info, bool async) {
  napi_value argv[7] = {nullptr};
  size_t argc = 7;
  CompressionHandle* handle = UnwrapHandle(env, info, nullptr, 7, &argc, argv);
  if (handle == nullptr || handle->context == nullptr || argc < 7) return Undefined(env);
  if (!handle->init_done || handle->closed || handle->write_in_progress) return Undefined(env);

  uint32_t flush = 0;
  if (!CoerceToUint32(env, argv[0], &flush)) return Undefined(env);

  ByteSpan input;
  if (!IsNullOrUndefined(env, argv[1]) && !ExtractByteSpan(env, argv[1], &input)) {
    return Undefined(env);
  }

  uint32_t in_off = 0;
  uint32_t in_len = 0;
  uint32_t out_off = 0;
  uint32_t out_len = 0;
  CoerceToUint32(env, argv[2], &in_off);
  CoerceToUint32(env, argv[3], &in_len);
  CoerceToUint32(env, argv[5], &out_off);
  CoerceToUint32(env, argv[6], &out_len);

  ByteSpan output;
  if (!ExtractByteSpan(env, argv[4], &output) || output.data == nullptr) return Undefined(env);
  if (in_off > input.len || in_len > input.len - in_off) return Undefined(env);
  if (out_off > output.len || out_len > output.len - out_off) return Undefined(env);

  handle->write_in_progress = true;
  handle->context->SetBuffers(
      input.data != nullptr ? input.data + in_off : nullptr,
      in_len,
      output.data + out_off,
      out_len);
  handle->context->SetFlush(flush);

  if (!async) {
    handle->context->DoWork();
    ReportExternalMemory(handle);
    const CompressionError err = handle->context->GetErrorInfo();
    if (err.IsError()) {
      EmitError(handle, err);
      return Undefined(env);
    }
    UpdateWriteResult(handle);
    handle->write_in_progress = false;
    if (handle->pending_close) CloseHandle(handle);
    return Undefined(env);
  }

  if (!StartAsyncWork(handle)) {
    handle->write_in_progress = false;
    napi_throw_error(env, nullptr, "Failed to queue zlib work");
    return nullptr;
  }
  return Undefined(env);
}

napi_value CompressionWrite(napi_env env, napi_callback_info info) {
  return CompressionWriteCommon(env, info, true);
}

napi_value CompressionWriteSync(napi_env env, napi_callback_info info) {
  return CompressionWriteCommon(env, info, false);
}

napi_value ZlibCrc32(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  const uint8_t* data = nullptr;
  size_t len = 0;
  std::string temp_utf8;
  if (!ExtractBinarySequence(env, argv[0], &data, &len, &temp_utf8)) return Undefined(env);

  uint32_t initial = 0;
  if (argc >= 2 && argv[1] != nullptr) {
    CoerceToUint32(env, argv[1], &initial);
  }

  napi_value out = nullptr;
  napi_create_uint32(
      env, static_cast<uint32_t>(crc32(initial, reinterpret_cast<const Bytef*>(data), len)), &out);
  return out != nullptr ? out : Undefined(env);
}

bool DefineCompressionClass(napi_env env,
                            napi_value target,
                            const char* name,
                            napi_callback ctor) {
  napi_property_descriptor methods[] = {
      {"init", nullptr, CompressionInit, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"params", nullptr, CompressionParams, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"reset", nullptr, CompressionReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, CompressionClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"write", nullptr, CompressionWrite, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeSync", nullptr, CompressionWriteSync, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, CompressionGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType",
       nullptr,
       CompressionGetProviderType,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"asyncReset", nullptr, CompressionAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };
  napi_value ctor_value = nullptr;
  if (napi_define_class(env,
                        name,
                        NAPI_AUTO_LENGTH,
                        ctor,
                        nullptr,
                        sizeof(methods) / sizeof(methods[0]),
                        methods,
                        &ctor_value) != napi_ok ||
      ctor_value == nullptr) {
    return false;
  }
  return napi_set_named_property(env, target, name, ctor_value) == napi_ok;
}

}  // namespace

napi_value InitZlib(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  napi_value crc32 = nullptr;
  if (napi_create_function(env, "crc32", NAPI_AUTO_LENGTH, ZlibCrc32, nullptr, &crc32) == napi_ok &&
      crc32 != nullptr) {
    napi_set_named_property(env, out, "crc32", crc32);
  }

  napi_value version = nullptr;
  if (napi_create_string_utf8(env, ZLIB_VERSION, NAPI_AUTO_LENGTH, &version) == napi_ok &&
      version != nullptr) {
    napi_set_named_property(env, out, "ZLIB_VERSION", version);
  }

  DefineCompressionClass(env, out, "Zlib", ZlibCtor);
  DefineCompressionClass(env, out, "BrotliEncoder", BrotliEncoderCtor);
  DefineCompressionClass(env, out, "BrotliDecoder", BrotliDecoderCtor);
  DefineCompressionClass(env, out, "ZstdCompress", ZstdCompressCtor);
  DefineCompressionClass(env, out, "ZstdDecompress", ZstdDecompressCtor);

  return out;
}

}  // namespace internal_binding
