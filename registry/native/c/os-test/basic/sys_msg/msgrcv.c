/*[XSI]*/
/* Test whether a basic msgrcv invocation works. */

#include <sys/msg.h>

#include <signal.h>

#include "../basic.h"

struct message
{
	long type;
	char c;
};

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
	struct message to_send = { .type = 42, .c = 'x' };
	size_t len = sizeof(to_send) - sizeof(long);
	if ( msgsnd(msgid, &to_send, len, 0) < 0 )
		err(1, "msgsnd");
	struct message to_recv;
	ssize_t amount = msgrcv(msgid, &to_recv, len, 42, 0);
	if ( amount < 0 )
		err(1, "msgrcv");
	if ( (size_t) amount != len )
		errx(1, "msgrcv returned wrong value");
	if ( to_recv.type != 42 )
		errx(1, "msgrcv gave wrong type");
	if ( to_recv.c != 'x' )
		errx(1, "msgrcv gave wrong data");
	return 0;
}
