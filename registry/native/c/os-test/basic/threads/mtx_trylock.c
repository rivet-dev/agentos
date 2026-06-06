/* Test whether a basic mtx_trylock invocation works. */

#include <threads.h>

#include "../basic.h"

int main(void)
{
	mtx_t mtx;
	int ret = mtx_init(&mtx, mtx_plain);
	if ( ret != thrd_success )
		errx(1, "mtx_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_trylock(&mtx);
	if ( ret != thrd_success )
		errx(1, "first mtx_trylock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_trylock(&mtx);
	if ( ret != thrd_busy )
		errx(1, "second mtx_timedlock: %s",
		     ret == thrd_success ? "thrd_success" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_unlock(&mtx);
	if ( ret != thrd_success )
		errx(1, "mtx_unlock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_trylock(&mtx);
	if ( ret != thrd_success )
		errx(1, "third mtx_trylock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	return 0;
}
