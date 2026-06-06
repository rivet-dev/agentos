/*[TSA TSS]*/
/* Test whether a basic pthread_attr_getstack invocation works. */

#include <sys/mman.h>

#include <limits.h>
#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	void* got_stack = NULL;
	size_t got_size = 0;
	if ( (errno = pthread_attr_getstack(&attr, &got_stack, &got_size)) )
		err(1, "pthread_attr_getstack");
	if ( got_stack != NULL )
		errx(1, "default stack was non-NULL");
	// TODO: What is the initial thread stack size supposed to be?
	if ( got_size < PTHREAD_STACK_MIN )
		errx(1, "default stack size < PTHREAD_STACK_MIN (%zu)", got_size);
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
	if ( (errno = pthread_attr_getstack(&attr, &got_stack, &got_size)) )
		err(1, "pthread_attr_getstack");
	if ( got_stack != stack )
		errx(1, "got wrong stack");
	if ( got_size != size )
		errx(1, "got wrong stack size");
	return 0;
}
