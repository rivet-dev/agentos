/* Test whether a basic pthread_cleanup_push invocation works. */

#include <pthread.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

static int fds[2];

static bool cleaned_up = false;

static void cleanup(void* ctx)
{
	(void) ctx;
	cleaned_up = true;
}

static void* start(void* ctx)
{
	pthread_cleanup_push(cleanup, NULL);
	char c;
	read(fds[0], &c, 1);
	pthread_cleanup_pop(0);
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
	if ( !cleaned_up )
		errx(1, "cleanup handler was not executed");
	return 0;
}
