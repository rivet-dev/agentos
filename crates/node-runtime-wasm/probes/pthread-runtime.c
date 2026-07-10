#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>

typedef int (*probe_function)(int);

typedef struct {
  pthread_mutex_t mutex;
  pthread_cond_t condition;
  int ready;
  int release;
  probe_function function;
} probe_context;

static pthread_key_t probe_tls_key;
static _Atomic int probe_tls_destructors;

static int probe_indirect_function(int value) {
  return value + 1;
}

static void probe_tls_destructor(void* value) {
  if ((uintptr_t)value == 0x1234u) {
    atomic_fetch_add_explicit(&probe_tls_destructors, 1, memory_order_relaxed);
  }
}

static void* probe_worker(void* opaque) {
  probe_context* context = opaque;
  if (pthread_setspecific(probe_tls_key, (void*)(uintptr_t)0x1234u) != 0) {
    return (void*)(intptr_t)-1;
  }
  if (pthread_mutex_lock(&context->mutex) != 0) return (void*)(intptr_t)-2;
  context->ready = 1;
  pthread_cond_signal(&context->condition);
  while (!context->release) {
    if (pthread_cond_wait(&context->condition, &context->mutex) != 0) {
      pthread_mutex_unlock(&context->mutex);
      return (void*)(intptr_t)-3;
    }
  }
  pthread_mutex_unlock(&context->mutex);
  return (void*)(intptr_t)context->function(41);
}

static void* probe_cancel_worker(void* opaque) {
  probe_context* context = opaque;
  pthread_mutex_lock(&context->mutex);
  context->ready = 1;
  pthread_cond_signal(&context->condition);
  pthread_mutex_unlock(&context->mutex);
  for (;;) {
    pthread_testcancel();
    sched_yield();
  }
}

// Returns zero only when the production pthread implementation proves every
// lifecycle operation. Each nonzero bit identifies the failed stage.
__attribute__((export_name("agentos_pthread_probe_run")))
uint32_t agentos_pthread_probe_run(void) {
  uint32_t failures = 0;
  probe_context context = {
      .mutex = PTHREAD_MUTEX_INITIALIZER,
      .condition = PTHREAD_COND_INITIALIZER,
      .ready = 0,
      .release = 0,
      .function = probe_indirect_function,
  };
  atomic_store_explicit(&probe_tls_destructors, 0, memory_order_relaxed);

  if (pthread_key_create(&probe_tls_key, probe_tls_destructor) != 0) return 1u << 0;
  pthread_t worker;
  if (pthread_create(&worker, 0, probe_worker, &context) != 0) {
    pthread_key_delete(probe_tls_key);
    return 1u << 1;
  }
  pthread_mutex_lock(&context.mutex);
  while (!context.ready) pthread_cond_wait(&context.condition, &context.mutex);
  context.release = 1;
  pthread_cond_signal(&context.condition);
  pthread_mutex_unlock(&context.mutex);

  void* result = 0;
  if (pthread_join(worker, &result) != 0) failures |= 1u << 2;
  if ((intptr_t)result != 42) failures |= 1u << 3;
  if (atomic_load_explicit(&probe_tls_destructors, memory_order_relaxed) != 1) {
    failures |= 1u << 4;
  }
  if (pthread_key_delete(probe_tls_key) != 0) failures |= 1u << 5;
  if (pthread_cond_destroy(&context.condition) != 0) failures |= 1u << 6;
  if (pthread_mutex_destroy(&context.mutex) != 0) failures |= 1u << 7;

  probe_context cancel_context = {
      .mutex = PTHREAD_MUTEX_INITIALIZER,
      .condition = PTHREAD_COND_INITIALIZER,
      .ready = 0,
      .release = 0,
      .function = probe_indirect_function,
  };
  pthread_t cancelled_worker;
  if (pthread_create(&cancelled_worker, 0, probe_cancel_worker, &cancel_context) != 0) {
    failures |= 1u << 8;
  } else {
    pthread_mutex_lock(&cancel_context.mutex);
    while (!cancel_context.ready) {
      pthread_cond_wait(&cancel_context.condition, &cancel_context.mutex);
    }
    pthread_mutex_unlock(&cancel_context.mutex);
    if (pthread_cancel(cancelled_worker) != 0) failures |= 1u << 9;
    result = 0;
    if (pthread_join(cancelled_worker, &result) != 0) failures |= 1u << 10;
    if (result != PTHREAD_CANCELED) failures |= 1u << 11;
  }
  if (pthread_cond_destroy(&cancel_context.condition) != 0) failures |= 1u << 12;
  if (pthread_mutex_destroy(&cancel_context.mutex) != 0) failures |= 1u << 13;
  return failures;
}
