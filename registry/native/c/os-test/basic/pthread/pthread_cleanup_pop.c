/* Test whether a basic pthread_cleanup_pop invocation works. */

#include <pthread.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

static int fds[2];

static bool cleaned_up1 = false;
static bool cleaned_up2 = false;
static bool cleaned_up3 = false;

static void cleanup1(void* ctx)
{
	(void) ctx;
	cleaned_up1 = true;
}

static void cleanup2(void* ctx)
{
	(void) ctx;
	cleaned_up2 = true;
}

static void cleanup3(void* ctx)
{
	(void) ctx;
	cleaned_up3 = true;
}

static void* start(void* ctx)
{
	pthread_cleanup_push(cleanup1, NULL);
	pthread_cleanup_push(cleanup2, NULL);
	pthread_cleanup_push(cleanup3, NULL);
	if ( cleaned_up1 || cleaned_up2 || cleaned_up3 )
		errx(1, "control test failed");
	pthread_cleanup_pop(0);
	if ( cleaned_up3 )
	{
		pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
		errx(1, "cleanup3 was not supposed to run");
	}
	pthread_cleanup_pop(1);
	if ( !cleaned_up2 )
	{
		pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, NULL);
		errx(1, "cleanup2 was supposed to run");
	}
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
	if ( !cleaned_up1 )
		errx(1, "cleanup1 was supposed to run");
	if ( !cleaned_up2 )
		errx(1, "cleanup2 was supposed to run");
	if ( cleaned_up3 )
		errx(1, "cleanup3 was not supposed to run");
	return 0;
}
