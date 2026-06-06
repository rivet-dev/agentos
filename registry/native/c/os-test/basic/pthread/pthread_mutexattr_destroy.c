/* Test whether a basic pthread_mutexattr_destroy invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_destroy(&attr)) )
		err(1, "pthread_mutexattr_destroy");
	return 0;
}
