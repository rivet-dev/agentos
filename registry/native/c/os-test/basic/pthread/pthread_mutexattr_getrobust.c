/* Test whether a basic pthread_mutexattr_getrobust invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	int robustness;
	if ( (errno = pthread_mutexattr_getrobust(&attr, &robustness)) )
		err(1, "pthread_mutexattr_getrobust");
	if ( robustness != PTHREAD_MUTEX_STALLED )
		errx(1, "mutex was not non-robust by default");
	return 0;
}
