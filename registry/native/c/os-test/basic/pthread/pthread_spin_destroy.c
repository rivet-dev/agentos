/* Test whether a basic pthread_spin_destroy invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_spinlock_t lock;
	if ( (errno = pthread_spin_init(&lock, 0)) )
		err(1, "pthread_spin_init");
	if ( (errno = pthread_spin_destroy(&lock)) )
		err(1, "pthread_spin_destroy");
	return 0;
}
