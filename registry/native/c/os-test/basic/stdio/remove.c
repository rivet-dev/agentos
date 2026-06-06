/* Test whether a basic remove invocation works. */

#include <sys/stat.h>

#include <stdio.h>
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
	// Create a temporary directory.
	char* tmpdir = create_tmpdir();
	// Put a file inside inside it.
	char* a = malloc(strlen(tmpdir) + 2 + 1);
	if ( !a )
	{
		warn("malloc");
		rmdir(tmpdir);
		exit(1);
	}
	strcpy(a, tmpdir);
	strcat(a, "/a");
	FILE* afp = fopen(a, "w");
	if ( !afp )
	{
		warn("fopen: tmpdir/a");
		unlink(a);
		rmdir(tmpdir);
		exit(1);
	}
	fclose(afp);
	// Test if remove can delete a file.
	if ( remove(a) < 0 )
	{
		warn("remove: tmpdir/a");
		unlink(a);
		rmdir(tmpdir);
		exit(1);
	}
	// Test if remove can delete a directory.
	if ( remove(tmpdir) < 0 )
	{
		warn("remove: tmpdir");
		rmdir(tmpdir);
		exit(1);
	}
	return 0;
}
