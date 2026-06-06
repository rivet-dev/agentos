/*[XSI]*/
/* Test whether a basic msgctl invocation works. */

#include <sys/msg.h>

#include <signal.h>
#include <unistd.h>

#include "../basic.h"

// XSI IPC resources are system wide and have no proper namespace. If we lose
// track of the id, there is no real way to know the purpose of the id, and
// nothing can safely reclaim it. To avoid leaks, this test uses atexit to make
// sure XSI IPC sources are cleaned up on process shutdown, and signal handlers
// are used to clean up on SIGINT/SIGQUIT/SIGTERM. However, resources will leak
// if the test crashes or is SIGKILL'd or otherwise abnormally terminated.
static int msgid;

static void cleanup(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	sigaddset(&set, SIGQUIT);
	sigaddset(&set, SIGTERM);
	sigprocmask(SIG_BLOCK, &set, &oldset);
	if ( 0 < msgid )
		msgctl(msgid, IPC_RMID, NULL);
	msgid = 0;
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
	sigaddset(&sa.sa_mask, SIGQUIT);
	sigaddset(&sa.sa_mask, SIGTERM);
	if ( sigaction(SIGINT, &sa, NULL) < 0 ||
	     sigaction(SIGQUIT, &sa, NULL) < 0 ||
	     sigaction(SIGTERM, &sa, NULL) < 0 )
	     err(1, "sigaction");
	msgid = msgget(IPC_PRIVATE, 0600);
	if ( msgid < 0 )
		err(1, "msgget");
	struct msqid_ds ds;
	if ( msgctl(msgid, IPC_STAT, &ds) < 0 )
		err(1, "msgctl");
	if ( ds.msg_perm.cuid != getuid() )
		errx(1, "wrong uid");
	if ( ds.msg_perm.uid != getuid() )
		errx(1, "wrong uid");
	if ( ds.msg_perm.gid != getgid() )
		errx(1, "wrong cgid");
	if ( ds.msg_perm.gid != getgid() )
		errx(1, "wrong gid");
	if ( ds.msg_perm.mode & 0777 != 0600 )
		errx(1, "wrong mode 0%o != 0600", ds.msg_perm.mode & 0777);
	if ( ds.msg_qnum != 0 )
		errx(1, "wrong msg_qnum");
	if ( ds.msg_lspid != 0 )
		errx(1, "wrong msg_lspid");
	if ( ds.msg_lrpid != 0 )
		errx(1, "wrong msg_lrpid");
	if ( ds.msg_stime != 0 )
		errx(1, "wrong msg_stime");
	if ( ds.msg_rtime != 0 )
		errx(1, "wrong msg_rtime");
	return 0;
}
