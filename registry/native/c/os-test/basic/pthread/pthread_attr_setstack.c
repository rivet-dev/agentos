/*[TSA TSS]*/
/* Test whether a basic pthread_attr_setstack invocation works. */

#include <sys/mman.h>

#include <limits.h>
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
	long page_size = sysconf(_SC_PAGESIZE);
	if ( page_size < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	size_t size = PTHREAD_STACK_MIN;
	size = -(-size & ~(page_size-1)); // Align
	void* stack = mmap(NULL, size, PROT_READ | PROT_WRITE,
	                   MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
	if ( stack == MAP_FAILED )
		err(1, "mmap");
	if ( (errno = pthread_attr_setstack(&attr, stack, size)) )
		err(1, "pthread_attr_setstack");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, &attr, start, NULL)) )
		err(1, "pthread_create");
	pthread_exit(NULL);
}
