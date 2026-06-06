/* Test whether a basic pthread_cond_broadcast invocation works. */

#include <pthread.h>
#include <sched.h>

#include "../basic.h"

static pthread_cond_t cnd_sync;
static pthread_cond_t cnd_wake;
static pthread_mutex_t mtx;
static int waiting = 0;
static int woken = 0;

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
	// Let the main thread know when all threads are asleep in pthread_cond_wait.
	if ( ++waiting == 2 )
	{
		if ( (errno = pthread_cond_signal(&cnd_sync)) )
			err(1, "pthread_cond_signal");
	}
	// Wait for the main thread to wake all threads with pthread_cond_broadcast.
	while ( !woken )
	{
		if ( (errno = pthread_cond_wait(&cnd_wake, &mtx)) )
			err(1, "pthread_cond_wait");
	}
	unlock(&mtx);
	return ctx;
}

int main(void)
{
	if ( (errno = pthread_cond_init(&cnd_sync, NULL)) )
		err(1, "pthread_cond_wait");
	if ( (errno = pthread_cond_init(&cnd_wake, NULL)) )
		err(1, "pthread_cond_wait");
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	int id1 = 1, id2 = 2;
	pthread_t thrd1, thrd2;
	if ( (errno = pthread_create(&thrd1, NULL, start, &id1)) )
		err(1, "pthread_create");
	if ( (errno = pthread_create(&thrd2, NULL, start, &id2)) )
		err(1, "pthread_create");
	lock(&mtx);
	// Wait for all threads to be asleep in pthread_cond_wait.
	while ( waiting < 2 )
	{
		if ( (errno = pthread_cond_wait(&cnd_sync, &mtx)) )
			err(1, "pthread_cond_wait");
	}
	// Test that all threads awake with pthread_cond_broadcast.
	woken = 1;
	if ( (errno = pthread_cond_broadcast(&cnd_wake)) )
		err(1, "pthread_cond_broadcast");
	unlock(&mtx);
	void* code;
	if ( (errno = pthread_join(thrd1, &code)) )
		err(1, "pthread_cond_broadcast");
	if ( (errno = pthread_join(thrd2, &code)) )
		err(1, "pthread_cond_broadcast");
	return 0;
}
