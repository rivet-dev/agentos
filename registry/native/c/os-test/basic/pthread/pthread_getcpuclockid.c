/*[TCT]*/
/* Test whether a basic pthread_getcpuclockid invocation works. */

#include <pthread.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

static void* start(void* ctx)
{
	while ( true )
		sleep(1);
	return ctx;
}

int main(void)
{
	clockid_t clock_id;
	if ( (errno = pthread_getcpuclockid(pthread_self(), &clock_id)) )
		err(1, "pthread_getcpuclockid self");
	struct timespec ts;
	if ( clock_gettime(clock_id, &ts) < 0 )
		err(1, "clock_gettime self cpu clock");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_getcpuclockid(thread, &clock_id)) )
		err(1, "pthread_getcpuclockid thread");
	if ( clock_gettime(clock_id, &ts) < 0 )
		err(1, "clock_gettime self cpu clock");
	return 0;
}
