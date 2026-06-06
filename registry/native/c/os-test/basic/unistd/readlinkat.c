/* Test whether a basic readlinkat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
#include <limits.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static char* create_tmpdir(void)
{
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	// mkdtemp is unfortunately less portable than link, so emulate it.
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
		if ( mkdir(template, 0700) < 0 )
		{
			if ( errno == EEXIST )
				continue;
			err(1, "mkdir");
		}
		break;
	}
	return template;
}

int main(void)
{
	char* tmpdir = create_tmpdir();
	char* dst = malloc(strlen(tmpdir) + 2 + 1);
	if ( !dst )
	{
		warn("malloc");
		rmdir(tmpdir);
		exit(1);
	}
	int tmpdir_fd = open(tmpdir, O_RDONLY | O_DIRECTORY);
	if ( tmpdir_fd < 0 )
		err(1, "open: tmpdir");
	if ( symlinkat("foo", tmpdir_fd, "a") < 0 )
	{
		warn("symlinkat");
		rmdir(tmpdir);
		exit(1);
	}
	char buffer[10];
	ssize_t amount = readlinkat(tmpdir_fd, "a", buffer, sizeof(buffer));
	if ( amount < 0 )
	{
		warn("readlinkat");
		unlinkat(tmpdir_fd, "a", 0);
		rmdir(tmpdir);
		exit(1);
	}
	buffer[amount] = 0;
	if ( strcmp(buffer, "foo") != 0 )
	{
		warn("readlinkat gave wrong contents");
		unlinkat(tmpdir_fd, "a", 0);
		rmdir(tmpdir);
		exit(1);
	}
	unlinkat(tmpdir_fd, "a", 0);
	rmdir(tmpdir);
	return 0;
}
