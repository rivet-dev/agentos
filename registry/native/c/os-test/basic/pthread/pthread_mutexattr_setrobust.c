/* Test whether a basic pthread_mutexattr_setrobust invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_setrobust(&attr, PTHREAD_MUTEX_ROBUST)) )
		err(1, "pthread_mutexattr_setrobust");
	int robustness;
	if ( (errno = pthread_mutexattr_getrobust(&attr, &robustness)) )
		err(1, "pthread_mutexattr_getrobust");
	if ( robustness != PTHREAD_MUTEX_ROBUST )
		errx(1, "mutex did not become robust");
	return 0;
}
