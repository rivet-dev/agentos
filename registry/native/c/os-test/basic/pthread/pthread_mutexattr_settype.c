/* Test whether a basic pthread_mutexattr_settype invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_RECURSIVE)) )
		err(1, "pthread_mutexattr_settype");
	pthread_mutex_t mutex;
	if ( (errno = pthread_mutex_init(&mutex, &attr)) )
		err(1, "pthread_mutex_init");
	if ( (errno = pthread_mutex_lock(&mutex)) )
		err(1, "first pthread_mutex_lock");
	if ( (errno = pthread_mutex_lock(&mutex)) )
		err(1, "second pthread_mutex_lock");
	return 0;
}
