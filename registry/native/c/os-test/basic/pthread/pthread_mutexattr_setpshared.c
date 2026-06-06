/*[TSH]*/
/* Test whether a basic pthread_mutexattr_setpshared invocation works. */

#include <sys/mman.h>
#include <sys/wait.h>

#include <pthread.h>
#include <signal.h>
#include <unistd.h>

#include "../basic.h"

static int in[2];
static int out[2];

int main(void)
{
	if ( pipe(in) < 0 || pipe(out) < 0 )
		err(1, "pipe");
	pthread_mutexattr_t attr;
	if ( (errno = pthread_mutexattr_init(&attr)) )
		err(1, "pthread_mutexattr_init");
	int shared = PTHREAD_PROCESS_SHARED;
	if ( (errno = pthread_mutexattr_setpshared(&attr, shared)) )
		err(1, "pthread_mutexattr_setpshared");
	long page_size = sysconf(_SC_PAGESIZE);
	if ( page_size < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	size_t size = sizeof(pthread_mutex_t);
	size = -(-size & ~(page_size-1)); // Align
	void* ptr = mmap(NULL, page_size, PROT_READ | PROT_WRITE,
	                 MAP_ANONYMOUS | MAP_SHARED, -1, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	pthread_mutex_t* mutex = ptr;
	if ( (errno = pthread_mutex_init(mutex, &attr)) )
		err(1, "pthread_mutex_init");
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	char c = 'x';
	if ( pid )
	{
		close(in[0]);
		close(out[1]);
		if ( (errno = pthread_mutex_lock(mutex)) )
		{
			kill(pid, SIGKILL);
			err(1, "parent pthread_mutex_lock");
		}
		write(in[1], &c, 1);
		read(out[0], &c, 1);
		if ( (errno = pthread_mutex_unlock(mutex)) )
		{
			kill(pid, SIGKILL);
			err(1, "parent pthread_mutex_unlock");
		}
		write(in[1], &c, 1);
	}
	else
	{
		close(in[1]);
		close(out[0]);
		read(in[0], &c, 1);
		if ( (errno = pthread_mutex_trylock(mutex)) )
		{
			if ( errno != EBUSY )
				err(1, "child pthread_mutex_trylock");
		}
		else
			errx(1, "child pthread_mutex_trylock did not fail");
		write(out[1], &c, 1);
		read(in[0], &c, 1);
		if ( (errno = pthread_mutex_lock(mutex)) )
			err(1, "child pthread_mutex_lock");
		if ( (errno = pthread_mutex_unlock(mutex)) )
			err(1, "child pthread_mutex_unlock");
	}
	if ( !pid )
		return 0;
	int status;
	if ( waitpid(pid, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
