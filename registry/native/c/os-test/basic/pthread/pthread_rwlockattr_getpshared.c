/*[TSH]*/
/* Test whether a basic pthread_rwlockattr_getpshared invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlockattr_t attr;
	if ( (errno = pthread_rwlockattr_init(&attr)) )
		err(1, "pthread_rwlockattr_init");
	int shared;
	if ( (errno = pthread_rwlockattr_getpshared(&attr, &shared)) )
		err(1, "pthread_rwlockattr_getpshared");
	if ( shared != PTHREAD_PROCESS_PRIVATE )
		errx(1, "rwlock was not private by default");
	return 0;
}
