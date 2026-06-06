/*[XSI]*/
/* Test whether a basic semctl invocation works. */

#include <sys/sem.h>

#include <signal.h>
#include <unistd.h>

#include "../basic.h"

union my_semun
{
	int val;
	struct semid_ds* buf;
	unsigned short* array;
};

// XSI IPC resources are system wide and have no proper namespace. If we lose
// track of the id, there is no real way to know the purpose of the id, and
// nothing can safely reclaim it. To avoid leaks, this test uses atexit to make
// sure XSI IPC sources are cleaned up on process shutdown, and signal handlers
// are used to clean up on SIGINT/SIGQUIT/SIGTERM. However, resources will leak
// if the test crashes or is SIGKILL'd or otherwise abnormally terminated.
static int semid;

static void cleanup(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	sigaddset(&set, SIGALRM);
	sigaddset(&set, SIGQUIT);
	sigaddset(&set, SIGTERM);
	sigprocmask(SIG_BLOCK, &set, &oldset);
	if ( 0 < semid )
		semctl(semid, 0, IPC_RMID, NULL);
	semid = 0;
	sigprocmask(SIG_SETMASK, &set, NULL);
}

static void on_signal(int signo)
{
	cleanup();
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, signo);
	raise(signo); // Make sure the signal is immediately pending on sigprocmask.
	sigprocmask(SIG_UNBLOCK, &set, NULL);
	raise(signo); // We should't end here, but try again.
}

int main(void)
{
	if ( atexit(cleanup) )
		err(1, "atexit");
	struct sigaction sa = { .sa_handler = on_signal };
	sigemptyset(&sa.sa_mask);
	sigaddset(&sa.sa_mask, SIGINT);
	sigaddset(&sa.sa_mask, SIGALRM);
	sigaddset(&sa.sa_mask, SIGQUIT);
	sigaddset(&sa.sa_mask, SIGTERM);
	if ( sigaction(SIGINT, &sa, NULL) < 0 ||
	     sigaction(SIGALRM, &sa, NULL) < 0 ||
	     sigaction(SIGQUIT, &sa, NULL) < 0 ||
	     sigaction(SIGTERM, &sa, NULL) < 0 )
	     err(1, "sigaction");
	semid = semget(IPC_PRIVATE, 1, 0600);
	if ( semid < 0 )
		err(1, "semget");
	struct semid_ds ds;
	union my_semun arg = { .buf = &ds };
	if ( semctl(semid, 0, IPC_STAT, arg) < 0 )
		err(1, "semctl");
	if ( ds.sem_perm.cuid != getuid() )
		errx(1, "wrong uid");
	if ( ds.sem_perm.uid != getuid() )
		errx(1, "wrong uid");
	if ( ds.sem_perm.gid != getgid() )
		errx(1, "wrong cgid");
	if ( ds.sem_perm.gid != getgid() )
		errx(1, "wrong gid");
	if ( ds.sem_perm.mode & 0777 != 0600 )
		errx(1, "wrong mode 0%o != 0600", ds.sem_perm.mode & 0777);
	if ( ds.sem_nsems != 1 )
		errx(1, "wrong sem_nsems");
	if ( ds.sem_otime != 0 )
		errx(1, "wrong sem_otime");
	return 0;
}
