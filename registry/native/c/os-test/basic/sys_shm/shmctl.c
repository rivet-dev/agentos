/*[XSI]*/
/* Test whether a basic shmctl invocation works. */

#include <sys/shm.h>

#include <signal.h>
#include <unistd.h>

#include "../basic.h"

// XSI IPC resources are system wide and have no proper namespace. If we lose
// track of the id, there is no real way to know the purpose of the id, and
// nothing can safely reclaim it. To avoid leaks, this test uses atexit to make
// sure XSI IPC sources are cleaned up on process shutdown, and signal handlers
// are used to clean up on SIGINT/SIGQUIT/SIGTERM. However, resources will leak
// if the test crashes or is SIGKILL'd or otherwise abnormally terminated.
static int shmid;

static void cleanup(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	sigaddset(&set, SIGALRM);
	sigaddset(&set, SIGQUIT);
	sigaddset(&set, SIGTERM);
	sigprocmask(SIG_BLOCK, &set, &oldset);
	if ( 0 < shmid )
		shmctl(shmid, IPC_RMID, NULL);
	shmid = 0;
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
	long pagesize = sysconf(_SC_PAGESIZE);
	if ( pagesize < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	shmid = shmget(IPC_PRIVATE, pagesize, 0600);
	if ( shmid < 0 )
		err(1, "shmget");
	struct shmid_ds ds;
	if ( shmctl(shmid, IPC_STAT, &ds) < 0 )
		err(1, "shmctl");
	if ( ds.shm_perm.cuid != getuid() )
		errx(1, "wrong cuid");
	if ( ds.shm_perm.uid != getuid() )
		errx(1, "wrong uid");
	if ( ds.shm_perm.cgid != getgid() )
		errx(1, "wrong cgid");
	if ( ds.shm_perm.gid != getgid() )
		errx(1, "wrong gid");
	if ( ds.shm_perm.mode & 0777 != 0600 )
		errx(1, "wrong mode 0%o != 0600", ds.shm_perm.mode & 0777);
	if ( ds.shm_lpid != 0 )
		errx(1, "wrong shm_lpid");
	if ( ds.shm_cpid != getpid() )
		errx(1, "wrong shm_cpid");
	if ( ds.shm_nattch != 0 )
		errx(1, "wrong shm_nattch");
	return 0;
}
