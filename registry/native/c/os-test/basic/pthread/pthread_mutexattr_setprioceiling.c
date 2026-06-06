/*[RPP|TPP]*/
/* Test whether a basic pthread_mutexattr_setprioceiling invocation works. */

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
	if ( (errno = pthread_mutexattr_setprioceiling(&attr, 31)) )
		err(1, "pthread_mutexattr_setprioceiling");
	return 0;
}
