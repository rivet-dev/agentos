/* Test whether a basic mtx_init invocation works. */

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
	return 0;
}
