/* Test whether a basic pthread_spin_trylock invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_spinlock_t lock;
	if ( (errno = pthread_spin_init(&lock, 0)) )
		err(1, "pthread_spin_init");
	if ( (errno = pthread_spin_trylock(&lock)) )
		err(1, "pthread_spin_trylock");
	if ( (errno = pthread_spin_trylock(&lock)) )
	{
		if ( errno != EBUSY )
			err(1, "pthread_spin_trylock");
	}
	else
		errx(1, "pthread_spin_trylock was not busy");
	if ( (errno = pthread_spin_unlock(&lock)) )
		err(1, "pthread_spin_unlock");
	if ( (errno = pthread_spin_trylock(&lock)) )
		err(1, "pthread_spin_trylock");
	return 0;
}
