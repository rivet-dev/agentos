/* Test whether a basic pthread_setcanceltype invocation works. */

#include <pthread.h>
#include <stdint.h>
#include <unistd.h>

#include "../basic.h"

static void* start(void* ctx)
{
	(void) ctx;
	int old_state;
	if ( (errno = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &old_state)) )
		err(1, "pthread_setcanceltype asynchronous");
	if ( old_state != PTHREAD_CANCEL_DEFERRED )
		err(1, "initial thread cancel type was not deferred");
	// Run for an extremely long time, without invoking any system calls, but
	// don't solve the halting problem as running forever without side effects
	// is technically undefined behavior. Make sure cancelation happens even
	// without any cancellation points in asynchronous mode.
	unsigned char c = 0;
	for ( int a = 0; a < 65536; a++ )
		for ( int b = 0; b < 65536; b++ )
			for ( int c = 0; c < 65536; c++ )
				for ( int d = 0; d < 65536; d++ )
					c += (a * b + c * d) ^ c;
	return (void*) (uintptr_t) c;
}

int main(void)
{
	alarm(1); // Haiku.
	int old_state;
	if ( (errno = pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, &old_state)) )
		err(1, "pthread_setcanceltype deferred");
	if ( old_state != PTHREAD_CANCEL_DEFERRED )
		err(1, "initial main cancel type was not deferred");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_cancel(thread)) )
		err(1, "pthread_cancel");
	void* result;
	if ( (errno = pthread_join(thread, &result)) )
		err(1, "pthread_cancel");
	if ( result != PTHREAD_CANCELED )
		errx(1, "pthread_join() != PTHREAD_CANCELED");
	return 0;
}
