/* Test whether a basic cnd_broadcast invocation works. */

#include <threads.h>

#include "../basic.h"

static cnd_t cnd_sync;
static cnd_t cnd_wake;
static mtx_t mtx;
static int waiting = 0;
static int woken = 0;

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
	int id = *((int*) ctx);
	thrd_yield();
	(void) ctx;
	lock(&mtx);
	// Let the main thread know when all threads are asleep in cnd_wait.
	if ( ++waiting == 2 )
	{
		int ret = cnd_signal(&cnd_sync);
		if ( ret != thrd_success )
			errx(1, "thread cnd_signal: %s",
				 ret == thrd_busy ? "thrd_busy" :
				 ret == thrd_nomem ? "thrd_nomem" :
				 ret == thrd_timedout ? "thrd_timedout" :
				 ret == thrd_error ? "thrd_error" :
				 "thrd_unknown");
	}
	// Wait for the main thread to wake all threads with cnd_broadcast.
	while ( !woken )
	{
		int ret = cnd_wait(&cnd_wake, &mtx);
		if ( ret != thrd_success )
			errx(1, "main cnd_wait: %s",
				 ret == thrd_busy ? "thrd_busy" :
				 ret == thrd_nomem ? "thrd_nomem" :
				 ret == thrd_timedout ? "thrd_timedout" :
				 ret == thrd_error ? "thrd_error" :
				 "thrd_unknown");
	}
	unlock(&mtx);
	return id;
}

int main(void)
{
	int ret = cnd_init(&cnd_sync);
	if ( ret != thrd_success )
		errx(1, "first cnd_init: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = cnd_init(&cnd_wake);
	if ( ret != thrd_success )
		errx(1, "second cnd_init: %s",
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
	int id1 = 1, id2 = 2;
	thrd_t thrd1, thrd2;
	ret = thrd_create(&thrd1, start, &id1);
	if ( ret != thrd_success )
		errx(1, "first thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = thrd_create(&thrd2, start, &id2);
	if ( ret != thrd_success )
		errx(1, "second thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	lock(&mtx);
	// Wait for all threads to be asleep in cnd_wait.
	while ( waiting < 2 )
	{
		ret = cnd_wait(&cnd_sync, &mtx);
		if ( ret != thrd_success )
			errx(1, "main cnd_wait: %s",
				 ret == thrd_busy ? "thrd_busy" :
				 ret == thrd_nomem ? "thrd_nomem" :
				 ret == thrd_timedout ? "thrd_timedout" :
				 ret == thrd_error ? "thrd_error" :
				 "thrd_unknown");
	}
	// Test that all threads awake with cnd_broadcast.
	woken = 1;
	cnd_broadcast(&cnd_wake);
	unlock(&mtx);
	int code;
	ret = thrd_join(thrd1, &code);
	if ( ret != thrd_success )
		errx(1, "first thrd_join: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	ret = thrd_join(thrd2, &code);
	if ( ret != thrd_success )
		errx(1, "second thrd_join: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	return 0;
}
