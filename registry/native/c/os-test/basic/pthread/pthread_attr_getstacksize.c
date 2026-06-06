/*[TSS]*/
/* Test whether a basic pthread_attr_getstacksize invocation works. */

#include <limits.h>
#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	size_t got_size = 0;
	if ( (errno = pthread_attr_getstacksize(&attr, &got_size)) )
		err(1, "pthread_attr_getstacksize");
	// TODO: What is the initial thread stack size supposed to be?
	if ( got_size < PTHREAD_STACK_MIN )
		errx(1, "default stack size < PTHREAD_STACK_MIN (%zu)", got_size);
	size_t size = PTHREAD_STACK_MIN;
	if ( (errno = pthread_attr_setstacksize(&attr, size)) )
		err(1, "pthread_attr_setstack");
	if ( (errno = pthread_attr_getstacksize(&attr, &got_size)) )
		err(1, "pthread_attr_getstacksize");
	if ( got_size != size )
		err(1, "got wrong stack size");
	return 0;
}
