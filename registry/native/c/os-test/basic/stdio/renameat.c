/* Test whether a basic renameat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
#include <stdio.h>
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
	int tmpdir_fd = open(tmpdir, O_RDONLY | O_DIRECTORY);
	if ( tmpdir_fd < 0 )
		err(1, "open: tmpdir");
	int src_fd = openat(tmpdir_fd, "a", O_WRONLY | O_CREAT, 0600);
	if ( src_fd < 0 )
	{
		warn("creat: tmpdir/a");
		rmdir(tmpdir);
		exit(1);
	}
	if ( renameat(tmpdir_fd, "a", tmpdir_fd, "b") < 0 )
	{
		warn("renameat");
		unlinkat(tmpdir_fd, "a", 0);
		unlinkat(tmpdir_fd, "b", 0);
		rmdir(tmpdir);
		exit(1);
	}
	unlinkat(tmpdir_fd, "a", 0);
	unlinkat(tmpdir_fd, "b", 0);
	rmdir(tmpdir);
	return 0;
}
