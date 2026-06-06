/*[TPS]*/
/* Test whether a basic pthread_attr_setscope invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	int scope = PTHREAD_SCOPE_SYSTEM;
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	if ( (errno = pthread_attr_setscope(&attr, scope)) )
		err(1, "pthread_attr_setscope");
	return 0;
}
