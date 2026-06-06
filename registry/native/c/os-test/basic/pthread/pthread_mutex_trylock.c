/* Test whether a basic pthread_mutex_trylock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_mutex_t mtx;
	if ( (errno = pthread_mutex_init(&mtx, NULL)) )
		err(1, "pthread_mutex_init");
	if ( (errno = pthread_mutex_trylock(&mtx)) )
		err(1, "pthread_mutex_trylock");
	if ( (errno = pthread_mutex_trylock(&mtx)) )
	{
		if ( errno != EBUSY )
			err(1, "pthread_mutex_trylock");
	}
	else
		errx(1, "pthread_mutex_trylock was not busy");
	if ( (errno = pthread_mutex_unlock(&mtx)) )
		err(1, "pthread_mutex_unlock");
	if ( (errno = pthread_mutex_trylock(&mtx)) )
		err(1, "pthread_mutex_trylock");
	return 0;
}
