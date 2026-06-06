/*[MC1]*/
/* Test whether a basic pthread_mutexattr_getprotocol invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	int protocol;
	if ( (errno = pthread_mutexattr_getprotocol(&attr, &protocol)) )
		err(1, "pthread_mutexattr_getprotocol");
	if ( protocol != PTHREAD_PRIO_NONE )
		errx(1, "the default protocol was not none");
	return 0;
}
