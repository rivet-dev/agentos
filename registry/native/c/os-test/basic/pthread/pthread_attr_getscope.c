/*[TPS]*/
/* Test whether a basic pthread_attr_getscope invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	int scope;
	if ( (errno = pthread_attr_getscope(&attr, &scope)) )
		err(1, "pthread_attr_getscope");
	if ( scope != PTHREAD_SCOPE_SYSTEM )
		errx(1, "default scope was not system");
	return 0;
}
