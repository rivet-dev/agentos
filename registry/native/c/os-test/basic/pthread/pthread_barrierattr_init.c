/* Test whether a basic pthread_barrierattr_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_barrierattr_t attr;
	if ( (errno = pthread_barrierattr_init(&attr)) )
		err(1, "pthread_barrierattr_init");
	return 0;
}
