/*[RPP|TPP]*/
/* Test whether a basic pthread_mutex_setprioceiling invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_setprotocol(&attr, PTHREAD_PRIO_PROTECT)) )
		err(1, "pthread_mutexattr_setprotocol");
	pthread_mutex_t mutex;
	if ( (errno = pthread_mutex_init(&mutex, &attr)) )
		err(1, "pthread_mutex_init");
	int old;
	if ( (errno = pthread_mutex_setprioceiling(&mutex, 31, &old)) )
		err(1, "pthread_mutex_setprioceiling");
	return 0;
}
