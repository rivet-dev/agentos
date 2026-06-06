/*[TSH]*/
/* Test whether a basic pthread_mutexattr_getpshared invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	int shared;
	if ( (errno = pthread_mutexattr_getpshared(&attr, &shared)) )
		err(1, "pthread_mutexattr_getpshared");
	if ( shared != PTHREAD_PROCESS_PRIVATE )
		errx(1, "mutex was not private by default");
	return 0;
}
