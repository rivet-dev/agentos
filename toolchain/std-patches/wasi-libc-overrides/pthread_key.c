/**
 * Fix for wasi-libc's broken pthread_key_delete in single-threaded WASM.
 *
 * Root cause: pthread_key_delete walks the thread list via a circular
 * linked list (td->next), but __wasilibc_pthread_self is zero-initialized
 * so self->next == NULL. The loop:
 *   do td->tsd[k] = 0; while ((td=td->next)!=self);
 * dereferences NULL on the second iteration, causing an infinite loop
 * or trap in WASM linear memory.
 *
 * Fix: In single-threaded WASM there's only one thread, so we just clear
 * self->tsd[k] directly — no thread list walk needed.
 *
 * This override replaces the entire pthread_key compilation unit (create,
 * delete, tsd_run_dtors) because they share a static keys[] array.
 * Uses musl internal headers (via -I flags) for struct __pthread access.
 */

#define hidden __attribute__((__visibility__("hidden")))
#include "pthread_impl.h"

volatile size_t __pthread_tsd_size = sizeof(void *) * PTHREAD_KEYS_MAX;
void *__pthread_tsd_main[PTHREAD_KEYS_MAX] = { 0 };

static void (*keys[PTHREAD_KEYS_MAX])(void *);

static pthread_rwlock_t key_lock = PTHREAD_RWLOCK_INITIALIZER;

static pthread_key_t next_key;

static void nodtor(void *dummy)
{
}

int __pthread_key_create(pthread_key_t *k, void (*dtor)(void *))
{
	pthread_t self = __pthread_self();

	/* This can only happen in the main thread before
	 * pthread_create has been called. */
	if (!self->tsd) self->tsd = __pthread_tsd_main;

	/* Purely a sentinel value since null means slot is free. */
	if (!dtor) dtor = nodtor;

	pthread_rwlock_wrlock(&key_lock);
	pthread_key_t j = next_key;
	do {
		if (!keys[j]) {
			keys[next_key = *k = j] = dtor;
			pthread_rwlock_unlock(&key_lock);
			return 0;
		}
	} while ((j=(j+1)%PTHREAD_KEYS_MAX) != next_key);

	pthread_rwlock_unlock(&key_lock);
	return EAGAIN;
}

int __pthread_key_delete(pthread_key_t k)
{
	/* POSIX: return EINVAL for out-of-range or unallocated keys */
	if (k >= PTHREAD_KEYS_MAX)
		return EINVAL;

	pthread_rwlock_wrlock(&key_lock);

	if (!keys[k]) {
		pthread_rwlock_unlock(&key_lock);
		return EINVAL;
	}

	pthread_t self = __pthread_self();

	/* Single-threaded WASM: only one thread exists, clear its TSD directly.
	 * Upstream musl walks td->next in a circular list, but
	 * __wasilibc_pthread_self.next is never initialized (zero = NULL),
	 * causing an infinite loop when dereferencing the NULL pointer. */
	if (self->tsd)
		self->tsd[k] = 0;

	keys[k] = 0;

	pthread_rwlock_unlock(&key_lock);

	return 0;
}

void __pthread_tsd_run_dtors(void)
{
	pthread_t self = __pthread_self();
	int i, j;
	for (j=0; self->tsd_used && j<PTHREAD_DESTRUCTOR_ITERATIONS; j++) {
		pthread_rwlock_rdlock(&key_lock);
		self->tsd_used = 0;
		for (i=0; i<PTHREAD_KEYS_MAX; i++) {
			void *val = self->tsd[i];
			void (*dtor)(void *) = keys[i];
			self->tsd[i] = 0;
			if (val && dtor && dtor != nodtor) {
				pthread_rwlock_unlock(&key_lock);
				dtor(val);
				pthread_rwlock_rdlock(&key_lock);
			}
		}
		pthread_rwlock_unlock(&key_lock);
	}
}

__attribute__((__weak__, __alias__("__pthread_key_create")))
int pthread_key_create(pthread_key_t *, void (*)(void *));

__attribute__((__weak__, __alias__("__pthread_key_delete")))
int pthread_key_delete(pthread_key_t);
