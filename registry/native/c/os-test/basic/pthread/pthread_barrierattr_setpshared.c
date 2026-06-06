/*[TSH]*/
/* Test whether a basic pthread_barrierattr_setpshared invocation works. */

#include <sys/mman.h>
#include <sys/wait.h>

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pthread_barrierattr_t attr;
	if ( (errno = pthread_barrierattr_init(&attr)) )
		err(1, "pthread_barrierattr_init");
	int shared = PTHREAD_PROCESS_SHARED;
	if ( (errno = pthread_barrierattr_setpshared(&attr, shared)) )
		err(1, "pthread_barrierattr_setpshared");
	long page_size = sysconf(_SC_PAGESIZE);
	if ( page_size < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	size_t size = sizeof(pthread_barrier_t);
	size = -(-size & ~(page_size-1)); // Align
	void* ptr = mmap(NULL, page_size, PROT_READ | PROT_WRITE,
	                 MAP_ANONYMOUS | MAP_SHARED, -1, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	pthread_barrier_t* barrier = ptr;
	if ( (errno = pthread_barrier_init(barrier, &attr, 2)) )
		err(1, "pthread_barrier_init");
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	if ( (errno = pthread_barrier_wait(barrier)) &&
	     errno != PTHREAD_BARRIER_SERIAL_THREAD )
		err(1, "%s pthread_barrier_wait", pid ? "parent" : "child");
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
