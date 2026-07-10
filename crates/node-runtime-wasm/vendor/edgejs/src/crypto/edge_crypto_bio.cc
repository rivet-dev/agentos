#include "crypto/edge_crypto_bio.h"

#include <algorithm>
#include <cassert>
#include <climits>
#include <cstring>

namespace edge::crypto {
namespace {

void AdjustExternalMemory(napi_env env, int64_t delta) {
  if (env == nullptr) return;
  int64_t adjusted = 0;
  (void)napi_adjust_external_memory(env, delta, &adjusted);
}

}  // namespace

EdgeBIO::Buffer::Buffer(napi_env env_in, size_t len_in)
    : env(env_in), len(len_in) {
  data = new char[len];
  AdjustExternalMemory(env, static_cast<int64_t>(len));
}

EdgeBIO::Buffer::~Buffer() {
  delete[] data;
  AdjustExternalMemory(env, -static_cast<int64_t>(len));
}

BIO* EdgeBIO::New(napi_env env) {
  BIO* bio = BIO_new(GetMethod());
  if (bio != nullptr && env != nullptr) {
    FromBIO(bio)->env_ = env;
  }
  return bio;
}

BIO* EdgeBIO::NewFixed(const char* data, size_t len, napi_env env) {
  BIO* bio = New(env);
  if (bio == nullptr || len > static_cast<size_t>(INT_MAX)) {
    if (bio != nullptr) BIO_free(bio);
    return nullptr;
  }

  if (WriteBio(bio, data, static_cast<int>(len)) != static_cast<int>(len) ||
      BIO_set_mem_eof_return(bio, 0) != 1) {
    BIO_free(bio);
    return nullptr;
  }

  return bio;
}

int EdgeBIO::NewBio(BIO* bio) {
  BIO_set_data(bio, new EdgeBIO());
  BIO_set_init(bio, 1);
  return 1;
}

int EdgeBIO::FreeBio(BIO* bio) {
  if (bio == nullptr) return 0;

  if (BIO_get_shutdown(bio) && BIO_get_init(bio) && BIO_get_data(bio) != nullptr) {
    delete FromBIO(bio);
    BIO_set_data(bio, nullptr);
  }

  return 1;
}

int EdgeBIO::ReadBio(BIO* bio, char* out, int len) {
  BIO_clear_retry_flags(bio);
  EdgeBIO* ebio = FromBIO(bio);
  int bytes = static_cast<int>(ebio->Read(out, len < 0 ? 0 : static_cast<size_t>(len)));
  if (bytes == 0) {
    bytes = ebio->eof_return();
    if (bytes != 0) BIO_set_retry_read(bio);
  }
  return bytes;
}

int EdgeBIO::WriteBio(BIO* bio, const char* data, int len) {
  BIO_clear_retry_flags(bio);
  if (len <= 0) return len;
  FromBIO(bio)->Write(data, static_cast<size_t>(len));
  return len;
}

int EdgeBIO::PutsBio(BIO* bio, const char* str) {
  return WriteBio(bio, str, static_cast<int>(std::strlen(str)));
}

int EdgeBIO::GetsBio(BIO* bio, char* out, int size) {
  EdgeBIO* ebio = FromBIO(bio);
  if (out == nullptr || size <= 0 || ebio->Length() == 0) return 0;

  int i = static_cast<int>(ebio->IndexOf('\n', static_cast<size_t>(size)));
  if (i < size && i >= 0 && static_cast<size_t>(i) < ebio->Length()) i++;
  if (size == i) i--;
  ebio->Read(out, static_cast<size_t>(i));
  out[i] = 0;
  return i;
}

long EdgeBIO::CtrlBio(BIO* bio, int cmd, long num, void* ptr) {
  EdgeBIO* ebio = FromBIO(bio);
  long ret = 1;

  switch (cmd) {
    case BIO_CTRL_RESET:
      ebio->Reset();
      break;
    case BIO_CTRL_EOF:
      ret = ebio->Length() == 0;
      break;
    case BIO_C_SET_BUF_MEM_EOF_RETURN:
      ebio->set_eof_return(static_cast<int>(num));
      break;
    case BIO_CTRL_INFO:
      ret = static_cast<long>(ebio->Length());
      if (ptr != nullptr) *reinterpret_cast<void**>(ptr) = nullptr;
      break;
    case BIO_CTRL_GET_CLOSE:
      ret = BIO_get_shutdown(bio);
      break;
    case BIO_CTRL_SET_CLOSE:
      BIO_set_shutdown(bio, static_cast<int>(num));
      break;
    case BIO_CTRL_WPENDING:
      ret = 0;
      break;
    case BIO_CTRL_PENDING:
      ret = static_cast<long>(ebio->Length());
      break;
    case BIO_CTRL_DUP:
    case BIO_CTRL_FLUSH:
      ret = 1;
      break;
    case BIO_CTRL_PUSH:
    case BIO_CTRL_POP:
    default:
      ret = 0;
      break;
  }

  return ret;
}

const BIO_METHOD* EdgeBIO::GetMethod() {
  static const BIO_METHOD* method = []() {
    BIO_METHOD* m = BIO_meth_new(BIO_TYPE_MEM, "edge TLS buffer");
    BIO_meth_set_write(m, WriteBio);
    BIO_meth_set_read(m, ReadBio);
    BIO_meth_set_puts(m, PutsBio);
    BIO_meth_set_gets(m, GetsBio);
    BIO_meth_set_ctrl(m, CtrlBio);
    BIO_meth_set_create(m, NewBio);
    BIO_meth_set_destroy(m, FreeBio);
    return m;
  }();
  return method;
}

void EdgeBIO::TryMoveReadHead() {
  while (read_head_ != nullptr &&
         read_head_->read_pos != 0 &&
         read_head_->read_pos == read_head_->write_pos) {
    read_head_->read_pos = 0;
    read_head_->write_pos = 0;
    if (read_head_ != write_head_) read_head_ = read_head_->next;
  }
}

size_t EdgeBIO::Read(char* out, size_t size) {
  size_t bytes_read = 0;
  size_t expected = std::min(Length(), size);
  size_t offset = 0;
  size_t left = size;

  while (bytes_read < expected) {
    assert(read_head_ != nullptr);
    assert(read_head_->read_pos <= read_head_->write_pos);
    size_t avail = read_head_->write_pos - read_head_->read_pos;
    if (avail > left) avail = left;

    if (out != nullptr) {
      std::memcpy(out + offset, read_head_->data + read_head_->read_pos, avail);
    }
    read_head_->read_pos += avail;
    bytes_read += avail;
    offset += avail;
    left -= avail;
    TryMoveReadHead();
  }

  assert(expected == bytes_read);
  length_ -= bytes_read;
  FreeEmpty();
  return bytes_read;
}

void EdgeBIO::FreeEmpty() {
  if (write_head_ == nullptr) return;
  Buffer* child = write_head_->next;
  if (child == write_head_ || child == read_head_) return;
  Buffer* cur = child->next;
  if (cur == write_head_ || cur == read_head_) return;

  Buffer* prev = child;
  while (cur != read_head_) {
    assert(cur != write_head_);
    assert(cur->write_pos == cur->read_pos);
    Buffer* next = cur->next;
    delete cur;
    cur = next;
  }
  prev->next = cur;
}

char* EdgeBIO::Peek(size_t* size) {
  if (size == nullptr) return nullptr;
  if (read_head_ == nullptr) {
    *size = 0;
    return nullptr;
  }
  *size = read_head_->write_pos - read_head_->read_pos;
  return read_head_->data + read_head_->read_pos;
}

size_t EdgeBIO::PeekMultiple(char** out, size_t* size, size_t* count) {
  if (out == nullptr || size == nullptr || count == nullptr || read_head_ == nullptr) {
    if (count != nullptr) *count = 0;
    return 0;
  }

  Buffer* pos = read_head_;
  size_t max = *count;
  size_t total = 0;
  size_t i = 0;
  for (; i < max; i++) {
    size[i] = pos->write_pos - pos->read_pos;
    total += size[i];
    out[i] = pos->data + pos->read_pos;
    if (pos == write_head_) break;
    pos = pos->next;
  }
  *count = (i == max) ? i : i + 1;
  return total;
}

size_t EdgeBIO::IndexOf(char delim, size_t limit) {
  size_t bytes_read = 0;
  size_t max = std::min(Length(), limit);
  size_t left = limit;
  Buffer* current = read_head_;

  while (bytes_read < max) {
    assert(current != nullptr);
    assert(current->read_pos <= current->write_pos);
    size_t avail = current->write_pos - current->read_pos;
    if (avail > left) avail = left;

    char* tmp = current->data + current->read_pos;
    size_t off = 0;
    while (off < avail && *tmp != delim) {
      off++;
      tmp++;
    }

    bytes_read += off;
    left -= off;

    if (off != avail) return bytes_read;
    if (current->read_pos + avail == current->len) current = current->next;
  }

  assert(max == bytes_read);
  return max;
}

void EdgeBIO::Write(const char* data, size_t size) {
  size_t offset = 0;
  size_t left = size;
  TryAllocateForWrite(left);

  while (left > 0) {
    assert(write_head_ != nullptr);
    size_t to_write = left;
    assert(write_head_->write_pos <= write_head_->len);
    size_t avail = write_head_->len - write_head_->write_pos;
    if (to_write > avail) to_write = avail;

    std::memcpy(write_head_->data + write_head_->write_pos, data + offset, to_write);
    left -= to_write;
    offset += to_write;
    length_ += to_write;
    write_head_->write_pos += to_write;
    assert(write_head_->write_pos <= write_head_->len);

    if (left != 0) {
      assert(write_head_->write_pos == write_head_->len);
      TryAllocateForWrite(left);
      write_head_ = write_head_->next;
      TryMoveReadHead();
    }
  }

  assert(left == 0);
}

char* EdgeBIO::PeekWritable(size_t* size) {
  if (size == nullptr) return nullptr;
  TryAllocateForWrite(*size);
  if (write_head_ == nullptr) {
    *size = 0;
    return nullptr;
  }
  size_t available = write_head_->len - write_head_->write_pos;
  if (*size == 0 || available <= *size) *size = available;
  return write_head_->data + write_head_->write_pos;
}

void EdgeBIO::Commit(size_t size) {
  assert(write_head_ != nullptr);
  write_head_->write_pos += size;
  length_ += size;
  assert(write_head_->write_pos <= write_head_->len);
  TryAllocateForWrite(0);
  if (write_head_->write_pos == write_head_->len) {
    write_head_ = write_head_->next;
    TryMoveReadHead();
  }
}

void EdgeBIO::TryAllocateForWrite(size_t hint) {
  Buffer* w = write_head_;
  Buffer* r = read_head_;
  if (w == nullptr ||
      (w->write_pos == w->len && (w->next == r || w->next->write_pos != 0))) {
    size_t len = w == nullptr ? initial_ : kThroughputBufferLength;
    if (len < hint) len = hint;
    if (allocate_hint_ > len) {
      len = allocate_hint_;
      allocate_hint_ = 0;
    }

    Buffer* next = new Buffer(env_, len);
    if (w == nullptr) {
      next->next = next;
      write_head_ = next;
      read_head_ = next;
    } else {
      next->next = w->next;
      w->next = next;
    }
  }
}

void EdgeBIO::Reset() {
  if (read_head_ == nullptr) return;
  while (read_head_->read_pos != read_head_->write_pos) {
    assert(read_head_->write_pos > read_head_->read_pos);
    length_ -= read_head_->write_pos - read_head_->read_pos;
    read_head_->write_pos = 0;
    read_head_->read_pos = 0;
    read_head_ = read_head_->next;
  }
  write_head_ = read_head_;
  assert(length_ == 0);
}

EdgeBIO::~EdgeBIO() {
  if (read_head_ == nullptr) return;
  Buffer* current = read_head_;
  do {
    Buffer* next = current->next;
    delete current;
    current = next;
  } while (current != read_head_);
  read_head_ = nullptr;
  write_head_ = nullptr;
}

EdgeBIO* EdgeBIO::FromBIO(BIO* bio) {
  assert(bio != nullptr);
  assert(BIO_get_data(bio) != nullptr);
  return static_cast<EdgeBIO*>(BIO_get_data(bio));
}

}  // namespace edge::crypto
