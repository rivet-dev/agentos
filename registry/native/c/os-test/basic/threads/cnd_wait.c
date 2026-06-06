/* Test whether a basic cnd_wait invocation works. */

#include <threads.h>

#include "../basic.h"

static cnd_t cnd;
static mtx_t mtx;
static int running = 0;

static void lock(mtx_t* mtx)
{
	int ret = mtx_lock(mtx);
	if ( ret != thrd_success )
		errx(1, "mtx_lock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
}

static void unlock(mtx_t* mtx)
{
	int ret = mtx_unlock(mtx);
	if ( ret != thrd_success )
		errx(1, "mtx_unlock: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
}

static int start(void* ctx)
{
	thrd_yield();
	(void) ctx;
	lock(&mtx);
	running = 1;
	int ret = cnd_signal(&cnd);
	if ( ret != thrd_success )
		errx(1, "cnd_signal: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	unlock(&mtx);
	return 0;
}

int main(void)
{
	int ret = cnd_init(&cnd);
	if ( ret != thrd_success )
		errx(1, "cnd_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = mtx_init(&mtx, mtx_plain);
	if ( ret != thrd_success )
		errx(1, "mtx_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	lock(&mtx);
	thrd_t thrd;
	ret = thrd_create(&thrd, start, NULL);
	if ( ret != thrd_success )
		errx(1, "thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	while ( !running )
	{
		ret = cnd_wait(&cnd, &mtx);
		if ( ret != thrd_success )
			errx(1, "cnd_wait: %s",
				 ret == thrd_busy ? "thrd_busy" :
				 ret == thrd_nomem ? "thrd_nomem" :
				 ret == thrd_timedout ? "thrd_timedout" :
				 ret == thrd_error ? "thrd_error" :
				 "thrd_unknown");
	}
	unlock(&mtx);
	return 0;
}
