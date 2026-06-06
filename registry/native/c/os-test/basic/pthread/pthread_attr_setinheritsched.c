/*[TPS]*/
/* Test whether a basic pthread_attr_setinheritsched invocation works. */

#include <pthread.h>
#include <sched.h>

#include "../basic.h"

static void* start(void* ctx)
{
	return ctx;
}

int main(void)
{
	int scope = PTHREAD_SCOPE_SYSTEM;
	int policy;
	struct sched_param param;
	if ( (errno = pthread_getschedparam(pthread_self(), &policy, &param)) )
		err(1, "pthread_getschedparam");
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	if ( (errno = pthread_attr_setscope(&attr, scope)) )
		err(1, "pthread_attr_setscope");
	if ( (errno = pthread_attr_setschedpolicy(&attr, policy)) )
		err(1, "pthread_attr_setschedpolicy");
	if ( (errno = pthread_attr_setschedparam(&attr, &param)) )
		err(1, "pthread_attr_setschedparam");
	if ( (errno = pthread_attr_setinheritsched(&attr, PTHREAD_EXPLICIT_SCHED)) )
		err(1, "pthread_attr_setinheritsched");
	pthread_t thread;
	if ( (errno = pthread_create(&thread, &attr, start, NULL)) )
		err(1, "pthread_create");
	pthread_exit(NULL);
}
