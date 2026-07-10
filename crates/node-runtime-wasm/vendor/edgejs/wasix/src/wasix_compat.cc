#include <errno.h>
#include <cstdlib>
#include <grp.h>
#include <stdint.h>
#include <openssl/thread.h>
#include <uv.h>

int getgroups(int size, gid_t* list) {
  if (size < 0) {
    errno = EINVAL;
    return -1;
  }
  (void)list;
  return 0;
}

extern "C" uint64_t uv_get_available_memory(void) {
  return 0;
}

extern "C" uint64_t uv_get_constrained_memory(void) {
  return 0;
}

extern "C" uint64_t uv_get_free_memory(void) {
  return 0;
}

extern "C" uint64_t uv_get_total_memory(void) {
  return 0;
}

extern "C" int uv_resident_set_memory(size_t* rss) {
  if (rss != nullptr) {
    *rss = 0;
  }
  return 0;
}

extern "C" int uv_cpu_info(uv_cpu_info_t** cpu_infos, int* count) {
  if (cpu_infos != nullptr) {
    *cpu_infos = nullptr;
  }
  if (count != nullptr) {
    *count = 0;
  }
  return -ENOSYS;
}

extern "C" int uv_interface_addresses(
    uv_interface_address_t** addresses,
    int* count) {
  if (addresses != nullptr) {
    *addresses = nullptr;
  }
  if (count != nullptr) {
    *count = 0;
  }
  return -ENOSYS;
}

extern "C" void uv_free_interface_addresses(
    uv_interface_address_t* addresses,
    int count) {
  (void)addresses;
  (void)count;
}

int OSSL_set_max_threads(OSSL_LIB_CTX* ctx, uint64_t max_threads) {
  (void)ctx;
  (void)max_threads;
  return 1;
}

extern "C" __attribute__((used, export_name("unofficial_napi_guest_malloc")))
uint32_t unofficial_napi_guest_malloc(uint32_t size) {
  void* ptr = std::malloc(static_cast<size_t>(size));
  return static_cast<uint32_t>(reinterpret_cast<uintptr_t>(ptr));
}
