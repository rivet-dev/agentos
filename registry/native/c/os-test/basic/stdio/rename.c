/* Test whether a basic rename invocation works. */

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
	// Create a temporary directory.
	char* tmpdir = create_tmpdir();
	// Put a file inside inside it.
	char* src = malloc(strlen(tmpdir) + 2 + 1);
	char* dst = malloc(strlen(tmpdir) + 2 + 1);
	if ( !src || !dst )
	{
		warn("malloc");
		rmdir(tmpdir);
		exit(1);
	}
	strcpy(src, tmpdir);
	strcat(src, "/a");
	strcpy(dst, tmpdir);
	strcat(dst, "/b");
	FILE* src_fp = fopen(src, "w");
	if ( !src_fp )
	{
		warn("fopen: tmpdir/a");
		unlink(src);
		rmdir(tmpdir);
		exit(1);
	}
	fclose(src_fp);
	// Test if the file can be renamed.
	if ( rename(src, dst) < 0 )
	{
		warn("rename");
		unlink(src);
		unlink(dst);
		rmdir(tmpdir);
		exit(1);
	}
	unlink(src);
	unlink(dst);
	rmdir(tmpdir);
	return 0;
}
