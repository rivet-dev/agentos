/* Test whether a basic pthread_rwlockattr_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_rwlockattr_t attr;
	if ( (errno = pthread_rwlockattr_init(&attr)) )
		err(1, "pthread_rwlockattr_init");
	return 0;
}
