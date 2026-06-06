/* Test whether a basic pthread_rwlock_rdlock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlock_t rwlock;
	if ( (errno = pthread_rwlock_init(&rwlock, NULL)) )
		err(1, "pthread_rwlock_init");
	if ( (errno = pthread_rwlock_rdlock(&rwlock)) )
		err(1, "pthread_rwlock_rdlock");
	return 0;
}
