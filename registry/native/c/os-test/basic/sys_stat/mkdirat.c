/* Test whether a basic mkdirat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
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
	int dirfd = open(tmpdir, O_RDONLY | O_DIRECTORY);
	if ( dirfd < 0 )
		err(1, "open: tmpdir");
	char* foo = malloc(strlen(tmpdir) + sizeof("/foo"));
	if ( !foo )
		err(1, "malloc");
	strcpy(foo, tmpdir);
	strcat(foo, "/foo");
	if ( mkdirat(dirfd, "foo", 0700) < 0 )
	{
		warn("mkdirat");
		rmdir(tmpdir);
		exit(1);
	}
	rmdir(foo);
	rmdir(tmpdir);
	return 0;
}
