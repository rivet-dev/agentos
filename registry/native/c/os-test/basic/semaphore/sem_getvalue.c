/* Test whether a basic sem_getvalue invocation works. */

#include <semaphore.h>

#include "../basic.h"

int main(void)
{
	sem_t sem;
	if ( sem_init(&sem, 0, 2) < 0 )
		err(1, "sem_init");
	int value;
	if ( sem_getvalue(&sem, &value) < 0 )
		err(1, "first sem_getvalue");
	if ( value != 2 )
		errx(1, "first sem_getvalue() != 2");
	if ( sem_trywait(&sem) < 0 )
		err(1, "first sem_trywait");
	if ( sem_getvalue(&sem, &value) < 0 )
		err(1, "second sem_getvalue");
	if ( value != 1 )
		errx(1, "second sem_getvalue() != 1");
	if ( sem_trywait(&sem) < 0 )
		err(1, "second sem_trywait");
	if ( sem_getvalue(&sem, &value) < 0 )
		err(1, "third sem_getvalue");
	if ( value != 0 )
		errx(1, "third sem_getvalue() != 0");
	return 0;
}
