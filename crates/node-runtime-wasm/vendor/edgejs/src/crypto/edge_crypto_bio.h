#ifndef EDGE_CRYPTO_BIO_H_
#define EDGE_CRYPTO_BIO_H_

#include <cstddef>

#include <openssl/bio.h>

#include "node_api.h"

namespace edge::crypto {

// EdgeBIO mirrors Node's NodeBIO so TLSWrap can use the same encrypted
// input/output ownership model that Node's native TLS implementation uses.
class EdgeBIO {
 public:
  ~EdgeBIO();

  static BIO* New(napi_env env = nullptr);
  static BIO* NewFixed(const char* data, size_t len, napi_env env = nullptr);
  static EdgeBIO* FromBIO(BIO* bio);

  void TryMoveReadHead();
  void TryAllocateForWrite(size_t hint);

  size_t Read(char* out, size_t size);
  void FreeEmpty();
  char* Peek(size_t* size);
  size_t PeekMultiple(char** out, size_t* size, size_t* count);
  size_t IndexOf(char delim, size_t limit);
  void Reset();
  void Write(const char* data, size_t size);
  char* PeekWritable(size_t* size);
  void Commit(size_t size);

  inline size_t Length() const { return length_; }

  inline void set_allocate_tls_hint(size_t size) {
    constexpr size_t kThreshold = 16 * 1024;
    if (size >= kThreshold) {
      allocate_hint_ = (size / kThreshold + 1) * (kThreshold + 5 + 32);
    }
  }

  inline void set_eof_return(int num) { eof_return_ = num; }
  inline int eof_return() const { return eof_return_; }
  inline void set_initial(size_t initial) { initial_ = initial; }

 private:
  struct Buffer {
    Buffer(napi_env env, size_t len);
    ~Buffer();

    napi_env env = nullptr;
    size_t read_pos = 0;
    size_t write_pos = 0;
    size_t len = 0;
    Buffer* next = nullptr;
    char* data = nullptr;
  };

  static int NewBio(BIO* bio);
  static int FreeBio(BIO* bio);
  static int ReadBio(BIO* bio, char* out, int len);
  static int WriteBio(BIO* bio, const char* data, int len);
  static int PutsBio(BIO* bio, const char* str);
  static int GetsBio(BIO* bio, char* out, int size);
  static long CtrlBio(BIO* bio, int cmd, long num, void* ptr);
  static const BIO_METHOD* GetMethod();

  static constexpr size_t kInitialBufferLength = 1024;
  static constexpr size_t kThroughputBufferLength = 16384;

  napi_env env_ = nullptr;
  size_t initial_ = kInitialBufferLength;
  size_t length_ = 0;
  size_t allocate_hint_ = 0;
  int eof_return_ = -1;
  Buffer* read_head_ = nullptr;
  Buffer* write_head_ = nullptr;
};

}  // namespace edge::crypto

#endif  // EDGE_CRYPTO_BIO_H_
