/*[TSH]*/
/* Test whether a basic pthread_condattr_setpshared invocation works. */

#include <sys/mman.h>
#include <sys/wait.h>

#include <signal.h>
#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

struct page
{
	pthread_cond_t cond;
	pthread_mutex_t mutex;
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
	// Make a shared cond variable.
	int shared = PTHREAD_PROCESS_SHARED;
	pthread_condattr_t condattr;
	if ( (errno = pthread_condattr_init(&condattr)) )
		err(1, "pthread_condattr_init");
	if ( (errno = pthread_condattr_setpshared(&condattr, shared)) )
		err(1, "pthread_condattr_setpshared");
	pthread_cond_t* cond = &page->cond;
	if ( (errno = pthread_cond_init(cond, &condattr)) )
		err(1, "pthread_cond_init");
	// Make a shared mutex.
	pthread_mutexattr_t mutexattr;
	if ( (errno = pthread_mutexattr_init(&mutexattr)) )
		err(1, "pthread_mutexattr_init");
	if ( (errno = pthread_mutexattr_setpshared(&mutexattr, shared)) )
		err(1, "pthread_mutexattr_setpshared");
	pthread_mutex_t* mutex = &page->mutex;
	if ( (errno = pthread_mutex_init(mutex, &mutexattr)) )
		err(1, "pthread_mutex_init");
	// Set up a handler to kill the child process if we died too.
	if ( atexit(exit_handler) )
		err(1, "atexit");
	// Fork.
	if ( (child_pid = fork()) < 0 )
		err(1, "fork");
	// Get the mutex in either process.
	if ( (errno = pthread_mutex_lock(mutex)) )
		err(1, "pthread_mutex_lock");
	// In the parent, set state to 1 and wait for state go to 2.
	if ( child_pid )
	{
		page->state = 1;
		if ( (errno = pthread_cond_signal(cond)) )
			err(1, "pthread_cond_signal");
		while ( page->state != 2 )
			if ( (errno = pthread_cond_wait(cond, mutex)) )
				err(1, "pthread_cond_wait");
	}
	// In the child, wait for state 1 and set state to 2.
	else
	{
		while ( page->state != 1 )
			if ( (errno = pthread_cond_wait(cond, mutex)) )
				err(1, "pthread_cond_wait");
		page->state = 2;
		if ( (errno = pthread_cond_signal(cond)) )
			err(1, "pthread_cond_signal");

	}
	// Release the mutex in either process.
	if ( (errno = pthread_mutex_unlock(mutex)) )
		err(1, "pthread_mutex_unlock");
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
