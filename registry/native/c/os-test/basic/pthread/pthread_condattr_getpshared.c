/*[TSH]*/
/* Test whether a basic pthread_condattr_getpshared invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_condattr_t attr;
	if ( (errno = pthread_condattr_init(&attr)) )
		err(1, "pthread_condattr_init");
	int shared;
	if ( (errno = pthread_condattr_getpshared(&attr, &shared)) )
		err(1, "pthread_condattr_getpshared");
	if ( shared != PTHREAD_PROCESS_PRIVATE )
		errx(1, "cond was not private by default");
	return 0;
}
