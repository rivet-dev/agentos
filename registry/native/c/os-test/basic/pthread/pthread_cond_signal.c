/* Test whether a basic pthread_cond_signal invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_cond_t cnd;
	if ( (errno = pthread_cond_init(&cnd, NULL)) )
		err(1, "pthread_cond_destroy");
	if ( (errno = pthread_cond_signal(&cnd)) )
		err(1, "pthread_cond_signal");
	return 0;
}
