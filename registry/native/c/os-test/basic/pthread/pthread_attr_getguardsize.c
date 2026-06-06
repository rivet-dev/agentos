/* Test whether a basic pthread_attr_getguardsize invocation works. */

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	long page_size = sysconf(_SC_PAGE_SIZE);
	if ( page_size < 0 )
		err(1, "sysconf: _SC_PAGE_SIZE");
	if ( (errno = pthread_attr_setguardsize(&attr, page_size)) )
		err(1, "pthread_attr_setguardsize");
	size_t size;
	if ( (errno = pthread_attr_getguardsize(&attr, &size)) )
		err(1, "pthread_attr_getguardsize");
	// Rounding upwards is allowed.
	if ( size < (size_t) page_size )
		errx(1, "guard size was set to less than requested");
	return 0;
}
