/*[OB]*/
/* Test whether a basic pthread_atfork invocation works. */

#include <sys/wait.h>

#include <pthread.h>
#include <unistd.h>

#include "../basic.h"

static pid_t parent_pid;
static int ran_prepare;
static int ran_parent;
static int ran_child;

static void prepare(void)
{
	if ( getpid() != parent_pid )
		errx(1, "prepare handler run outside parent process");
	if ( ran_prepare )
		errx(1, "prepare handler ran twice");
	ran_prepare++;
}

static void parent(void)
{
	if ( getpid() != parent_pid )
		errx(1, "parent handler run outside parent process");
	if ( ran_parent )
		errx(1, "parent handler ran twice");
	if ( !ran_prepare )
		errx(1, "parent handler ran without prepare handler first");
	ran_parent++;
}

static void child(void)
{
	if ( getpid() == parent_pid )
		errx(1, "child handler run inside parent process");
	if ( ran_child )
		errx(1, "child handler ran twice");
	if ( !ran_prepare )
		errx(1, "child handler ran without prepare handler first");
	ran_child++;
}

int main(void)
{
	parent_pid = getpid();
	if ( (errno = pthread_atfork(prepare, parent, child)) )
		err(1, "pthread_atfork");
	if ( ran_prepare || ran_parent || ran_child )
		errx(1, "handlers ran before fork");
	pid_t pid = fork();
	if ( pid < 0 )
		err(1, "fork");
	if ( !pid )
	{
		if ( !ran_prepare )
			errx(1, "prepare handler did not run in child");
		if ( !ran_child )
			errx(1, "child handler did not run in child");
		if ( ran_parent )
			errx(1, "parent handler ran in child");
		return 0;
	}
	if ( !ran_prepare )
		errx(1, "prepare handler did not run in parent");
	if ( ran_child )
		errx(1, "child handler ran in parent");
	if ( !ran_parent )
		errx(1, "parent handler did not ran in parent");
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
