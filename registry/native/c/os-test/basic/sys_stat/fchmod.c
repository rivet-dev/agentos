/* Test whether a basic fchmod invocation works. */

#include <sys/stat.h>

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

static const char* temporary;

static void cleanup(void)
{
	if ( temporary )
		unlink(temporary);
}

int main(void)
{
	if ( atexit(cleanup) )
		err(1, "atexit");
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	strcpy(template, tmpdir);
	strcat(template, "/os-test.XXXXXX");
	int fd = mkstemp(template);
	if ( fd < 0 )
		err(1, "mkstemp");
	temporary = template;
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "first fstat");
	if ( (st.st_mode & 07777) != 0600 )
		errx(1, "control: mkstemp did not use mode 0600");
	if ( fchmod(fd, 0400) < 0 )
		err(1, "fchmod");
	if ( fstat(fd, &st) < 0 )
		err(1, "second fstat");
	if ( (st.st_mode & 07777) != 0400 )
		errx(1, "fchmod did not change to mode 0400");
	return 0;
}
