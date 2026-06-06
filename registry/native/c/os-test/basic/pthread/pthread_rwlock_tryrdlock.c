/* Test whether a basic pthread_rwlock_tryrdlock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlock_t rwlock;
	if ( (errno = pthread_rwlock_init(&rwlock, NULL)) )
		err(1, "pthread_rwlock_init");
	if ( (errno = pthread_rwlock_tryrdlock(&rwlock)) )
		err(1, "first pthread_rwlock_tryrdlock");
	if ( (errno = pthread_rwlock_tryrdlock(&rwlock)) )
		err(1, "second pthread_rwlock_tryrdlock");
	if ( (errno = pthread_rwlock_trywrlock(&rwlock)) )
	{
		if ( errno != EBUSY && errno != EDEADLK )
			err(1, "pthread_rwlock_trywrlock");
	}
	else
		errx(1, "pthread_rwlock_trywrlock did not fail");
	return 0;
}
