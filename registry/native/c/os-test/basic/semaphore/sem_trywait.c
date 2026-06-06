/* Test whether a basic sem_trywait invocation works. */

#include <semaphore.h>

#include "../basic.h"

int main(void)
{
	sem_t sem;
	if ( sem_init(&sem, 0, 2) < 0 )
		err(1, "sem_init");
	if ( sem_trywait(&sem) < 0 )
		err(1, "first sem_trywait");
	if ( sem_trywait(&sem) < 0 )
		err(1, "second sem_trywait");
	if ( sem_trywait(&sem) < 0 )
	{
		if ( errno != EAGAIN )
			err(1, "third sem_trywait");
	}
	else
		errx(1, "third sem_trywait unexpectedly succeding");
	if ( sem_post(&sem) < 0 )
		err(1, "sem_post");
	if ( sem_trywait(&sem) < 0 )
		err(1, "fourth sem_trywait");
	return 0;
}
