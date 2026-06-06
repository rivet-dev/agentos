/*[MSG]*/
/* Test whether a basic mq_timedsend invocation works. */

#include <fcntl.h>
#include <mqueue.h>
#include <signal.h>
#include <time.h>
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
	struct mq_attr attr = { .mq_maxmsg = 1, .mq_msgsize = 3 };
	mqd_t mq = create_mq(&mq_path, &attr);

	// Fill the queue.	
	struct timespec now;
	clock_gettime(CLOCK_REALTIME, &now);
	if ( mq_timedsend(mq, "foo", 3, 0, &now) < 0 )
		err(1, "first mq_timedsend");

	// Try timing out when the queue is full.
	clock_gettime(CLOCK_REALTIME, &now);
	if ( mq_timedsend(mq, "foo", 3, 0, &now) < 0 )
	{
		if ( errno != ETIMEDOUT )
			err(1, "second mq_timedsend");
	}
	else
		errx(1, "second mq_timedsend did not ETIMEDOUT");

	// Try a negative tv_nsec.
	now.tv_nsec = -1;
	if ( mq_timedsend(mq, "foo", 3, 0, &now) < 0 )
	{
		if ( errno != EINVAL )
			err(1, "third mq_timedsend");
	}
	else
		errx(1, "third mq_timedsend did not EINVAL");

	// Try too large tv_nsec.
	now.tv_nsec = 1000000000L;
	if ( mq_timedsend(mq, "foo", 3, 0, &now) < 0 )
	{
		if ( errno != EINVAL )
			err(1, "fourth mq_timedsend");
	}
	else
		errx(1, "fourth mq_timedsend did not EINVAL");

	return 0;
}
