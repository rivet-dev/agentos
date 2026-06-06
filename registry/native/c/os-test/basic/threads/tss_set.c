/* Test whether a basic tss_set invocation works. */

#include <threads.h>

#include "../basic.h"

static tss_t tss;
static int id1 = 1, id2 = 2;
static int invoked = 0;

static void destructor(void* ptr)
{
	invoked = *((int*) ptr);
}

static int start(void* ctx)
{
	(void) ctx;
	if ( tss_get(tss) )
		errx(1, "thread tss_get returned non-null");
	int ret = tss_set(tss, &id2);
	if ( ret != thrd_success )
		errx(1, "tss_set: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( tss_get(tss) != &id2 )
		errx(1, "thread tss_get did not return id1");
	return 0;
}

int main(void)
{
	int ret = tss_create(&tss, destructor);
	if ( ret != thrd_success )
		errx(1, "tss_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( tss_get(tss) )
		errx(1, "main tss_get returned non-null");
	ret = tss_set(tss, &id1);
	if ( ret != thrd_success )
		errx(1, "tss_set: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( tss_get(tss) != &id1 )
		errx(1, "first main tss_get did not return id1");
	thrd_t thrd;
	ret = thrd_create(&thrd, start, NULL);
	if ( ret != thrd_success )
		errx(1, "thrd_create: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	int code;
	ret = thrd_join(thrd, &code);
	if ( ret != thrd_success )
		errx(1, "thrd_join: %s",
		     ret == thrd_busy ? "thrd_busy" :
		     ret == thrd_nomem ? "thrd_nomem" :
		     ret == thrd_timedout ? "thrd_timedout" :
		     ret == thrd_error ? "thrd_error" :
		     "thrd_unknown");
	if ( tss_get(tss) != &id1 )
		errx(1, "second main tss_get did not return id1");
	if ( invoked != id2 )
		errx(1, "destructor was not run");
	return 0;
}
