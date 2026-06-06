/*[TSH]*/
/* Test whether a basic pthread_rwlockattr_setpshared invocation works. */

#include <sys/mman.h>
#include <sys/wait.h>

#include <signal.h>
#include <stdbool.h>
#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

struct page
{
	pthread_rwlock_t rwlock;
	int state;
};

static pid_t child_pid;

static void exit_handler(void)
{
	if ( child_pid )
		kill(child_pid, SIGKILL);
}

int main(void)
{
#ifdef __sun__ /* Solaris */
	alarm(1);
#endif
	// Allocate a shared page.
	long page_size = sysconf(_SC_PAGESIZE);
	if ( page_size < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	size_t size = sizeof(struct page);
	size = -(-size & ~(page_size-1)); // Align
	void* ptr = mmap(NULL, page_size, PROT_READ | PROT_WRITE,
	                 MAP_ANONYMOUS | MAP_SHARED, -1, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	struct page* page = ptr;
	// Make a shared rwlock.
	int shared = PTHREAD_PROCESS_SHARED;
	pthread_rwlockattr_t rwlockattr;
	if ( (errno = pthread_rwlockattr_init(&rwlockattr)) )
		err(1, "pthread_rwlockattr_init");
	if ( (errno = pthread_rwlockattr_setpshared(&rwlockattr, shared)) )
		err(1, "pthread_rwlockattr_setpshared");
	pthread_rwlock_t* rwlock = &page->rwlock;
	if ( (errno = pthread_rwlock_init(rwlock, &rwlockattr)) )
		err(1, "pthread_rwlock_init");
	// Set up a handler to kill the child process if we died too.
	if ( atexit(exit_handler) )
		err(1, "atexit");
	// Fork.
	if ( (child_pid = fork()) < 0 )
		err(1, "fork");
	// In either process, see if we're in state 2 yet, otherwise try writing
	// to increase the state if we haven't already.
	bool increased = false;
	bool done = false;
	while ( true )
	{
		if ( (errno = pthread_rwlock_rdlock(rwlock)) )
			err(1, "pthread_rwlock_rdlock");
		if ( page->state == 2 )
			done = true;
		if ( (errno = pthread_rwlock_unlock(rwlock)) )
			err(1, "pthread_rwlock_unlock");
		if ( done )
			break;
		if ( !increased )
		{
			if ( (errno = pthread_rwlock_wrlock(rwlock)) )
				err(1, "pthread_rwlock_wrlock");
			page->state++;
			increased = false;
			if ( (errno = pthread_rwlock_unlock(rwlock)) )
				err(1, "pthread_rwlock_unlock");
		}
		sched_yield();
	}
	// Collect the child process.
	if ( !child_pid )
		return 0;
	int status;
	if ( waitpid(child_pid, &status, 0) < 0 )
		err(1, "waitpid");
	child_pid = 0;
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
