/* Test whether a basic pthread_condattr_destroy invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_condattr_t attr;
	if ( (errno = pthread_condattr_init(&attr)) )
		err(1, "pthread_condattr_init");
	if ( (errno = pthread_condattr_destroy(&attr)) )
		err(1, "pthread_condattr_destroy");
	return 0;
}
