/* Test whether a basic pthread_attr_getdetachstate invocation works. */

#include <pthread.h>

#include "../basic.h"

int main(void)
{
	pthread_attr_t attr;
	if ( (errno = pthread_attr_init(&attr)) )
		err(1, "pthread_attr_init");
	// Inspect the default state.
	int state;
	if ( (errno = pthread_attr_getdetachstate(&attr, &state)) )
		err(1, "pthread_attr_getdetachstate");
	if ( state != PTHREAD_CREATE_JOINABLE )
		errx(1, "default state was not joinable");
	if ( (errno = pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_JOINABLE)) )
		err(1, "pthread_attr_setdetachstate");
	// Check if the state can be set to detached.
	if ( (errno = pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED)) )
		err(1, "pthread_attr_setdetachstate detached");
	if ( (errno = pthread_attr_getdetachstate(&attr, &state)) )
		err(1, "pthread_attr_getdetachstate");
	if ( state != PTHREAD_CREATE_DETACHED )
		errx(1, "could not set detached state");
	// Check if the state can be restored to joinable.
	if ( (errno = pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_JOINABLE)) )
		err(1, "pthread_attr_setdetachstate detached");
	if ( (errno = pthread_attr_getdetachstate(&attr, &state)) )
		err(1, "pthread_attr_getdetachstate");
	if ( state != PTHREAD_CREATE_JOINABLE )
		errx(1, "could not set joinable state");
	return 0;
}
