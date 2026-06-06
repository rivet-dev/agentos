/* Test whether a basic sem_destroy invocation works. */

#include <semaphore.h>

#include "../basic.h"

int main(void)
{
	sem_t sem;
	if ( sem_init(&sem, 0, 1) < 0 )
		err(1, "sem_init");
	if ( sem_destroy(&sem) < 0 )
		err(1, "sem_destroy");
	return 0;
}
