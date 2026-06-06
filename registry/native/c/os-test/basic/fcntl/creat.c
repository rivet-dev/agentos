/* Test whether a basic creat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static char* temporary;

static void cleanup(void)
{
	if ( temporary )
		rmdir(temporary);
}

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
	if ( atexit(cleanup) )
		err(1, "atexit");
	temporary = create_tmpdir();
	const char* suffix = "/foo";
	char* file = malloc(strlen(temporary) + strlen(suffix) + 1);
	if ( !file )
		err(1, "malloc");
	strcpy(file, temporary);
	strcat(file, suffix);
	int fd = creat(file, 0600);
	if ( fd < 0 )
		err(1, "creat");
	unlink(file);
	return 0;
}
