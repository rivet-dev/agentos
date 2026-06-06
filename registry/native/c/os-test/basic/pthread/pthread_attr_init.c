/* Test whether a basic pthread_attr_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	return 0;
}
