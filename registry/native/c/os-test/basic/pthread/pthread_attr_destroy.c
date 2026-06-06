/* Test whether a basic pthread_attr_destroy invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	if ( (errno = pthread_attr_destroy(&attr)) )
		err(1, "pthread_attr_destroy");
	return 0;
}
