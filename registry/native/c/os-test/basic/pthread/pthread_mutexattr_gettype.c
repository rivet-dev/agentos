/* Test whether a basic pthread_mutexattr_gettype invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	int type;
	if ( (errno = pthread_mutexattr_gettype(&attr, &type)) )
		err(1, "pthread_mutexattr_gettype");
	if ( type != PTHREAD_MUTEX_DEFAULT )
		errx(1, "mutex type was not default by default");
	return 0;
}
