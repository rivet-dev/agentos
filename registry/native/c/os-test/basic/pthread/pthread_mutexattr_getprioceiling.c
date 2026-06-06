/*[RPP|TPP]*/
/* Test whether a basic pthread_mutexattr_getprioceiling invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	// POSIX doesn't require setprotocol to happen before setprioceiling, but
	// setprioceiling fails with EINVAL on DragonFly, FreeBSD, OmniOS, OpenBSD,
	// and Solaris without it.
	if ( (errno = pthread_mutexattr_setprotocol(&attr, PTHREAD_PRIO_PROTECT)) )
		err(1, "pthread_mutexattr_setprotocol");
	int priority;
	if ( (errno = pthread_mutexattr_getprioceiling(&attr, &priority)) )
		err(1, "pthread_mutexattr_getprioceiling");
	return 0;
}
