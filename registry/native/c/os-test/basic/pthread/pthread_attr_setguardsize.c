/* Test whether a basic pthread_attr_setguardsize invocation works. */

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

static void* start(void* ctx)
{
	return ctx;
}

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
	pthread_t thread;
	if ( (errno = pthread_create(&thread, &attr, start, NULL)) )
		err(1, "pthread_create");
	pthread_exit(NULL);
}
