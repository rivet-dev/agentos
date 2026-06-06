/* Test whether a basic pthread_cancel invocation works. */

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

static int fds[2];

static void* start(void* ctx)
{
	char c;
	read(fds[0], &c, 1);
	return ctx;
}

int main(void)
{
	alarm(1); // Haiku.
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_cancel(thread)) )
		err(1, "pthread_cancel");
	void* result;
	if ( (errno = pthread_join(thread, &result)) )
		err(1, "pthread_cancel");
	if ( result != PTHREAD_CANCELED )
		errx(1, "pthread_join() != PTHREAD_CANCELED");
	return 0;
}
