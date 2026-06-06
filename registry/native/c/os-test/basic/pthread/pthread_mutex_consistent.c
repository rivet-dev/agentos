/* Test whether a basic pthread_mutex_consistent invocation works. */

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

static pthread_mutex_t mutex;

static void* start(void* ctx)
{
	if ( (errno = pthread_mutex_lock(&mutex)) )
		err(1, "first pthread_mutex_lock");
	return ctx;
}

// breaks on hurd

int main(void)
{
	alarm(1); // Hurd.
	// Create a robust mutex.
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_setrobust(&attr, PTHREAD_MUTEX_ROBUST)) )
		err(1, "pthread_mutexattr_setrobust");
	if ( (errno = pthread_mutex_init(&mutex, &attr)) )
		err(1, "pthread_mutex_init");
	// Lock the lock in a terminated thread.
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	void* code;
	if ( (errno = pthread_join(thread, &code)) )
		err(1, "pthread_join");
	// Verify the robust mutex fails with EOWNERDEAD.
	if ( (errno = pthread_mutex_lock(&mutex)) )
	{
		if ( errno != EOWNERDEAD )
			err(1, "second pthread_mutex_lock");
	}
	else
		errx(1, "second pthread_mutex_lock did not fail");
	// Verify we got ownership after EOWNERDEAD.
	if ( (errno = pthread_mutex_trylock(&mutex)) )
	{
		if ( errno != EBUSY )
			err(1, "pthread_mutex_trylock");
	}
	else
		errx(1, "pthread_mutex_trylock did not fail");
	// Verify the mutex can be repaired.
	if ( (errno = pthread_mutex_consistent(&mutex)) )
		err(1, "pthread_mutex_consistent");
	if ( (errno = pthread_mutex_unlock(&mutex)) )
		err(1, "second pthread_mutex_unlock");
	if ( (errno = pthread_mutex_lock(&mutex)) )
		err(1, "second pthread_mutex_lock");
	return 0;
}
