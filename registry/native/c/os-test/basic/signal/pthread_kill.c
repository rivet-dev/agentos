/* Test whether a basic pthread_kill invocation works. */

#include <signal.h>
#include <pthread.h>

#include "../basic.h"

static pthread_t thread;
static sigset_t oldset;
static volatile sig_atomic_t received;

static void on_signal(int signo)
{
	received = signo;
	if ( pthread_self() != thread )
		errx(1, "signal handler in wrong thread");
}

static void* start(void* ctx)
{
	(void) ctx;
	sigsuspend(&oldset);
	return NULL;
}

int main(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( (errno = pthread_sigmask(SIG_BLOCK, &set, &oldset)) )
		err(1, "pthread_sigmask");
	struct sigaction sa = { .sa_handler = on_signal };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	if ( (errno = pthread_create(&thread, NULL, start, NULL)) )
		err(1, "pthread_create");
	if ( (errno = pthread_sigmask(SIG_SETMASK, &oldset, NULL)) )
		err(1, "pthread_sigmask");
	if ( (errno = pthread_kill(thread, SIGUSR1)) < 0 )
		err(1, "pthread_kill");
	void* result;
	if ( (errno = pthread_join(thread, &result)) < 0 )
		err(1, "pthread_join");
	if ( !received )
		errx(1, "signal was not received");
	return 0;
}
