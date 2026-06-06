/* Test whether a basic freopen invocation works. */

#include <sys/stat.h>

#include <stdio.h>
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
	// Create a temporary file.
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
	// Test redirecting stdout to the temporary file.
	FILE* fp = freopen(template, "w", stdout);
	if ( !fp )
		err(1, "freopen");
	// Write some data to stdout.
	int ret = printf("test\n");
	if ( ret < 0 )
		err(1, "printf");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	// Test whether the data ended up in the temporary file.
	struct stat st;
	if ( stat(template, &st) < 0 )
		err(1, "stat");
	if ( st.st_size != 5 )
		errx(1, "temporary file had wrong size");
	return 0;
}
