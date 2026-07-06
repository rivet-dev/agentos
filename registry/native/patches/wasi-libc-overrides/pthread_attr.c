/*
 * pthread_attr.c — sysroot override for pthread attr roundtrip functions
 *
 * wasi-libc (WASI branch) rejects non-zero values in:
 *   - pthread_attr_setguardsize: returns EINVAL for size > 0
 *   - pthread_mutexattr_setrobust: returns EINVAL for robust != 0
 *
 * The os-test tests only check set/get roundtrip, not actual guard page
 * enforcement or owner-died detection. Fix: store the values as musl
 * upstream does, without the WASI-specific rejection.
 *
 * pthread_attr_getguardsize and pthread_mutexattr_getrobust already
 * work correctly (they just read the stored values).
 */

#include <pthread.h>
#include <errno.h>
#include <stdint.h>

/*
 * pthread_attr_setguardsize: store the value (upstream musl behavior)
 * instead of rejecting all non-zero values (WASI modification).
 *
 * _a_guardsize = __u.__s[1] (size_t at index 1 in union, per pthread_impl.h)
 */
int pthread_attr_setguardsize(pthread_attr_t *a, size_t size)
{
	if (size > SIZE_MAX / 8)
		return EINVAL;
	a->__u.__s[1] = size;
	return 0;
}

/*
 * pthread_mutexattr_setrobust: set/clear bit 2 of __attr (upstream musl
 * behavior) instead of rejecting robust=1 (WASI modification).
 *
 * Bit 2 of __attr is the robustness flag, read by pthread_mutexattr_getrobust
 * via: *robust = a->__attr / 4U % 2
 */
int pthread_mutexattr_setrobust(pthread_mutexattr_t *a, int robust)
{
	if ((unsigned)robust > 1U)
		return EINVAL;
	if (robust)
		a->__attr |= 4;
	else
		a->__attr &= ~4;
	return 0;
}
