/**
 * Fix for wasi-libc's broken pthread mutex stub in single-threaded WASM.
 *
 * The wasi-libc stub-pthreads/mutex.c has a C operator precedence bug:
 *   if (m->_m_type&3 != PTHREAD_MUTEX_RECURSIVE)
 * parses as:
 *   if (m->_m_type & (3 != 1))   →   if (m->_m_type & 1)
 * which inverts NORMAL (type=0) and RECURSIVE (type=1) behavior:
 *   - NORMAL mutexes act as RECURSIVE (double-lock succeeds)
 *   - RECURSIVE mutexes act as NORMAL (double-lock returns EDEADLK)
 *
 * This override fixes the precedence and adds proper timedlock timeout
 * handling. Uses _m_count for lock tracking (matching the stub condvar's
 * expectation that _m_count != 0 means "mutex is held").
 *
 * Internal fields used:
 *   _m_type  = __u.__i[0]  (lower 2 bits = mutex type)
 *   _m_count = __u.__i[5]  (lock count: 0=unlocked, >0=locked)
 */

#include <pthread.h>
#include <errno.h>
#include <time.h>
#include <limits.h>

/* Field accessors matching musl's pthread_impl.h macros */
#define M_TYPE(m)    ((m)->__u.__i[0])
#define M_COUNT(m)   ((m)->__u.__i[5])

#define TYPE_MASK 3

int __pthread_mutex_trylock(pthread_mutex_t *m)
{
	int type = M_TYPE(m) & TYPE_MASK;

	if (type == PTHREAD_MUTEX_RECURSIVE) {
		if ((unsigned)M_COUNT(m) >= INT_MAX) return EAGAIN;
		M_COUNT(m)++;
		return 0;
	}

	/* NORMAL or ERRORCHECK */
	if (M_COUNT(m)) return EBUSY;
	M_COUNT(m) = 1;
	return 0;
}

int __pthread_mutex_timedlock(pthread_mutex_t *restrict m,
                              const struct timespec *restrict at)
{
	int type = M_TYPE(m) & TYPE_MASK;

	if (type == PTHREAD_MUTEX_RECURSIVE) {
		if ((unsigned)M_COUNT(m) >= INT_MAX) return EAGAIN;
		M_COUNT(m)++;
		return 0;
	}

	/* NORMAL or ERRORCHECK — single-threaded deadlock detection */
	if (M_COUNT(m))
		return at ? ETIMEDOUT : EDEADLK;

	M_COUNT(m) = 1;
	return 0;
}

int __pthread_mutex_lock(pthread_mutex_t *m)
{
	return __pthread_mutex_timedlock(m, 0);
}

int __pthread_mutex_unlock(pthread_mutex_t *m)
{
	if (!M_COUNT(m))
		return EPERM;
	M_COUNT(m)--;
	return 0;
}

int pthread_mutex_consistent(pthread_mutex_t *m)
{
	/* Robust mutexes not supported in single-threaded WASI */
	return EINVAL;
}

/* Weak aliases so both __pthread_mutex_* and pthread_mutex_* resolve */
__attribute__((__weak__, __alias__("__pthread_mutex_trylock")))
int pthread_mutex_trylock(pthread_mutex_t *);

__attribute__((__weak__, __alias__("__pthread_mutex_timedlock")))
int pthread_mutex_timedlock(pthread_mutex_t *restrict,
                            const struct timespec *restrict);

__attribute__((__weak__, __alias__("__pthread_mutex_lock")))
int pthread_mutex_lock(pthread_mutex_t *);

__attribute__((__weak__, __alias__("__pthread_mutex_unlock")))
int pthread_mutex_unlock(pthread_mutex_t *);
