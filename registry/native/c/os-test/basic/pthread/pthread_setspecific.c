/* Test whether a basic pthread_setspecific invocation works. */

#include <pthread.h>

#include "../basic.h"

static pthread_key_t tss;
static int id1 = 1, id2 = 2;
static int invoked = 0;

static void destructor(void* ptr)
{
	invoked = *((int*) ptr);
}

static void* start(void* ctx)
{
	(void) ctx;
	if ( pthread_getspecific(tss) )
		errx(1, "thread pthread_getspecific returned non-null");
	if ( (errno = pthread_setspecific(tss, &id2)) )
		err(1, "pthread_setspecific");
	if ( pthread_getspecific(tss) != &id2 )
		errx(1, "thread pthread_getspecific did not return id1");
	return 0;
}

int main(void)
{
	if ( (errno = pthread_key_create(&tss, destructor)) )
		err(1, "pthread_key_create");
	if ( pthread_getspecific(tss) )
		errx(1, "main pthread_getspecific returned non-null");
	if ( (errno = pthread_setspecific(tss, &id1)) )
		err(1, "pthread_setspecific");
	if ( pthread_getspecific(tss) != &id1 )
		errx(1, "first main pthread_getspecific did not return id1");
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, NULL)) )
		err(1, "pthread_create");
	void* code;
	if ( (errno = pthread_join(thrd, &code)) )
		err(1, "pthread_join");
	if ( pthread_getspecific(tss) != &id1 )
		errx(1, "second main pthread_getspecific did not return id1");
	if ( invoked != id2 )
		errx(1, "destructor was not run");
	return 0;
}
