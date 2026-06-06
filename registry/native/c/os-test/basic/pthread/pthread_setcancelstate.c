/* Test whether a basic pthread_setcancelstate invocation works. */

#include <pthread.h>
#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static bool canceled = false;
static bool at_cancellation_point = false;

static void* start(void* ctx)
{
	(void) ctx;
	// Disable cancellation immediately and test the initial state was correct.
	int old_state;
	if ( (errno = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &old_state)) )
		err(1, "pthread_setcancelstate");
	if ( old_state != PTHREAD_CANCEL_ENABLE )
		errx(1, "initial thread cancel state was disabled");
	// Wait to be canceled.
	pthread_mutex_lock(&mutex);
	while ( !canceled )
		pthread_cond_wait(&cond, &mutex);
	// Enable cancellation. This call is not a cancellation point.
	if ( (errno = pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, NULL)) )
		err(1, "pthread_setcancelstate");
	// Neither is this call.
	pthread_mutex_unlock(&mutex);
	// But sleep is a cancellation point. Be canceled here.
	at_cancellation_point = true;
	sleep(1);
	at_cancellation_point = false;
	return NULL;
}

int main(void)
{
	// Test the initial state.
	int old_state;
	if ( (errno = pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, &old_state)) )
		err(1, "pthread_setcancelstate deferred");
	if ( old_state != PTHREAD_CANCEL_ENABLE )
		errx(1, "initial main cancel state was disabled");
	// Create a thread to be canceled.
	pthread_t thread;
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	// Cancel the thread. This won't take effect immediately because the thread
	// disables cancellation before the first cancellation point.
	if ( (errno = pthread_cancel(thread)) )
		err(1, "pthread_cancel");
	// Notify the thread that cancellation is now pending, so we can proceed.
	pthread_mutex_lock(&mutex);
	pthread_cond_signal(&cond);
	canceled = true;
	pthread_mutex_unlock(&mutex);
	// Wait for the thread to be canceled.
	void* result;
	if ( (errno = pthread_join(thread, &result)) )
		err(1, "pthread_cancel");
	// Test the thread was canceled.
	if ( result != PTHREAD_CANCELED )
		errx(1, "pthread_join() != PTHREAD_CANCELED");
	// Test the thread was canceled at the correct location.
	if ( !at_cancellation_point )
		errx(1, "cancellation not at point");
	return 0;
}
