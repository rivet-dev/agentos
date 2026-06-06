/* Test whether a basic pthread_create invocation works. */

#include <pthread.h>
#include <sched.h>

#include "../basic.h"

static volatile int running = 1;
static int magic = 42;

static void* start(void* ctx)
{
	if ( *((int*) ctx) != 42 )
		errx(1, "start had wrong parameter");
	exit(0);
	running = 0;
}

int main(void)
{
	pthread_t thrd;
	if ( (errno = pthread_create(&thrd, NULL, start, &magic)) )
		err(1, "pthread_create");
	while ( running )
		sched_yield();
	return 1;
}
