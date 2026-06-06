/* Test whether a basic pthread_cond_wait invocation works. */

#include <pthread.h>

#include "../basic.h"

static pthread_cond_t cnd;
static pthread_mutex_t mtx;
static int running = 0;

static void lock(pthread_mutex_t* mtx)
{
	if ( (errno = pthread_mutex_lock(mtx)) )
		err(1, "pthread_mutex_lock");
}

static void unlock(pthread_mutex_t* mtx)
{
	if ( (errno = pthread_mutex_unlock(mtx)) )
		err(1, "pthread_mutex_unlock");
}

static void* start(void* ctx)
{
	sched_yield();
	lock(&mtx);
	running = 1;
	if ( (errno = pthread_cond_signal(&cnd)) )
		err(1, "pthread_cond_signal");
	unlock(&mtx);
	return ctx;
}

int main(void)
{
	if ( (errno = pthread_cond_init(&cnd, NULL)) )
		err(1, "pthread_cond_init");
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	lock(&mtx);
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, NULL)) )
		err(1, "pthread_create");
	while ( !running )
	{
		if ( (errno = pthread_cond_wait(&cnd, &mtx)) )
			err(1, "pthread_cond_wait");
	}
	unlock(&mtx);
	return 0;
}
