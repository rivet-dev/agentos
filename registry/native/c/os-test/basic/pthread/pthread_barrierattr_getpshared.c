/*[TSH]*/
/* Test whether a basic pthread_barrierattr_getpshared invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_barrierattr_t attr;
	if ( (errno = pthread_barrierattr_init(&attr)) )
		err(1, "pthread_barrierattr_init");
	int got_shared;
	if ( (errno = pthread_barrierattr_getpshared(&attr, &got_shared)) )
		err(1, "pthread_barrierattr_getpshared");
	if ( got_shared != PTHREAD_PROCESS_PRIVATE )
		errx(1, "default was not private");
	int shared = PTHREAD_PROCESS_SHARED;
	if ( (errno = pthread_barrierattr_setpshared(&attr, shared)) )
		err(1, "pthread_barrierattr_setpshared");
	if ( (errno = pthread_barrierattr_getpshared(&attr, &got_shared)) )
		err(1, "pthread_barrierattr_getpshared");
	if ( got_shared != PTHREAD_PROCESS_SHARED )
		errx(1, "could not share");
	return 0;
}
