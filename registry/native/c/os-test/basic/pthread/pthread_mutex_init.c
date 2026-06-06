/* Test whether a basic pthread_mutex_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutex_t mtx;
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	return 0;
}
