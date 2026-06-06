/* Test whether a basic pthread_mutex_lock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutex_t mtx;
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	if ( (errno = pthread_mutex_lock(&mtx)) )
		err(1, "pthread_mutex_lock");
	return 0;
}
