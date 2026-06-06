/* Test whether a basic mtx_timedlock invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	mtx_t mtx;
	int ret = mtx_init(&mtx, mtx_timed);
	if ( ret != thrd_success )
		errx(1, "mtx_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	struct timespec long_timeout;
	timespec_get(&long_timeout, TIME_UTC);
	long_timeout.tv_sec += 60;
	ret = mtx_timedlock(&mtx, &long_timeout);
	if ( ret != thrd_success )
		errx(1, "first mtx_timedlock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	struct timespec short_timeout;
	timespec_get(&short_timeout, TIME_UTC);
	ret = mtx_timedlock(&mtx, &short_timeout);
	if ( ret != thrd_timedout )
		errx(1, "second mtx_timedlock: %s",
		     ret == thrd_success ? "thrd_success" :
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	return 0;
}
