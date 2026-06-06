/*[XSI]*/
/* Test whether a basic utimes invocation works. */

#include <sys/stat.h>
#include <sys/time.h>

#include <fcntl.h>
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
	struct timeval times[2] =
	{
		{ .tv_sec = 2025, .tv_usec = 1 },
		{ .tv_sec = 2026, .tv_usec = 2 },
	};
	if ( utimes(temporary, times) < 0 )
		err(1, "utimes");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "fstat");
	if ( st.st_atim.tv_sec != times[0].tv_sec )
		errx(1, "utimes did not set atim");
	if ( st.st_mtim.tv_sec != times[1].tv_sec )
		errx(1, "utimes did not set mtim");
	return 0;
}
