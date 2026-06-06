/* Test whether a basic pthread_spin_init invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_spinlock_t lock;
	if ( (errno = pthread_spin_init(&lock, 0)) )
		err(1, "pthread_spin_init");
	return 0;
}
