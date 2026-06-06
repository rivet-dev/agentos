/*[MSG]*/
/* Test whether a basic mq_receive invocation works. */

#include <fcntl.h>
#include <mqueue.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

// Message queues are system wide resources. Make sure the queue is deleted upon
// exit or if the program is terminated by SIGINT/SIGQUIT/SIGTERM. All queues
// are given temorary random names with the template os-test.XXXXXX, so you can
// clean up any message queues that might somehow leak, and know where they come
// from. Message queues are in their own namespace, which may or may not be in
// the filesystem, and may or may not use file descriptors.
static char* mq_path;

static void cleanup(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGINT);
	sigaddset(&set, SIGALRM);
	sigaddset(&set, SIGQUIT);
	sigaddset(&set, SIGTERM);
	sigprocmask(SIG_BLOCK, &set, &oldset);
	if ( mq_path )
		mq_unlink(mq_path);
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

static mqd_t create_mq(char** out_path, const struct mq_attr* attr)
{
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	// Use mkstemp to generate random message queue names.
	while ( 1 )
	{
		strcpy(template, tmpdir);
		strcat(template, "/os-test.XXXXXX");
		int fd = mkstemp(template);
		if ( fd < 0 )
			err(1, "mkstemp");
		close(fd);
		if ( unlink(template) < 0 )
			err(1, "unlink");
		char* path = strdup(template + strlen(tmpdir));
		if ( !path )
			err(1, "malloc");
		mqd_t mq = mq_open(path, O_RDWR | O_CREAT | O_EXCL, 0600, attr);
		if ( mq == (mqd_t) -1 )
		{
			free(path);
			if ( errno == EEXIST )
				continue;
			err(1, "mq_open");
		}
		free(template);
		*out_path = path;
		return mq;
	}
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
	struct mq_attr attr = { .mq_maxmsg = 2, .mq_msgsize = 3 };
	mqd_t mq = create_mq(&mq_path, &attr);

	// Send a low priority message.
	if ( mq_send(mq, "foo", 3, 0) < 0 )
		err(1, "first mq_send");

	// Send a high priority message.
	if ( mq_send(mq, "bar", 3, 1) < 0 )
		err(1, "second mq_send");

	char buf[3];
	unsigned prio;
	ssize_t amount;

	// Test failing if the buffer is too small.
	amount = mq_receive(mq, buf, 1, &prio);
	if ( amount < 0 )
	{
		if ( errno != EMSGSIZE )
			err(1, "first mq_receive");
	}
	else
		errx(1, "first mq_receive did not EMSGSIZE");

	// Try receiving the highest priority message.
	amount = mq_receive(mq, buf, sizeof(buf), &prio);
	if ( amount < 0 )
		err(1, "second mq_receive");
	if ( amount != 3 )
		err(1, "second mq_receive() != 3");
	if ( prio != 1 )
		errx(1, "priority 1 was not received");
	if ( memcmp(buf, "bar", 3) != 0 )
		errx(1, "bar was not received");

	// Try receiving the second highest priority message.
	amount = mq_receive(mq, buf, sizeof(buf), NULL);
	if ( amount < 0 )
		err(1, "third mq_receive");
	if ( amount != 3 )
		err(1, "third mq_receive() != 3");
	if ( memcmp(buf, "foo", 3) != 0 )
		errx(1, "foo was not received");

	// Try receiving non-blocking when the queue is full.
	struct mq_attr new_attr = { .mq_flags = O_NONBLOCK };
	if ( mq_setattr(mq, &new_attr, NULL) < 0 )
		err(1, "mq_setattr");
	amount = mq_receive(mq, buf, sizeof(buf), &prio);
	if ( amount < 0 )
	{
		if ( errno != EAGAIN )
			err(1, "fourth mq_receive");
	}
	else
		errx(1, "fourth mq_receive did not EAGAIN");

	return 0;
}
