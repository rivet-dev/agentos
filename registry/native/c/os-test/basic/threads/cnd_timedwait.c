/* Test whether a basic cnd_timedwait invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	cnd_t cnd;
	int ret = cnd_init(&cnd);
	if ( ret != thrd_success )
		errx(1, "cnd_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	mtx_t mtx;
	ret = mtx_init(&mtx, mtx_timed);
	if ( ret != thrd_success )
		errx(1, "mtx_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_lock(&mtx);
	if ( ret != thrd_success )
		errx(1, "mtx_lock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	struct timespec short_timeout;
	timespec_get(&short_timeout, TIME_UTC);
	ret = cnd_timedwait(&cnd, &mtx, &short_timeout);
	if ( ret != thrd_timedout )
		errx(1, "cnd_timedwait: %s",
		     ret == thrd_success ? "thrd_success" :
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	return 0;
}
