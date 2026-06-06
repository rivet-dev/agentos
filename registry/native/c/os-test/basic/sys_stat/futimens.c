/* Test whether a basic futimens invocation works. */

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
	struct timespec times[2] =
	{
		{ .tv_sec = 2025, .tv_nsec = 1 },
		{ .tv_sec = 2026, .tv_nsec = 2 },
	};
	if ( futimens(fd, times) < 0 )
		err(1, "futimens");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "fstat");
	if ( st.st_atim.tv_sec != times[0].tv_sec )
		errx(1, "futimens did not set atim");
	if ( st.st_mtim.tv_sec != times[1].tv_sec )
		errx(1, "futimens did not set mtim");
	return 0;
}
