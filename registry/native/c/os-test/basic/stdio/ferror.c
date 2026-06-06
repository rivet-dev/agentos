/* Test whether a basic ferror invocation works. */

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
	// Open it as read-only.
	FILE* fp = fopen(template, "r");
	if ( !fp )
		err(1, "fopen");
	// Verify the error condiition was not set.
	if ( ferror(fp) )
		errx(1, "ferror unexpectedly on fresh file");
	// Create an error condition by trying to write to it.
	fputc('x', fp);
	fflush(fp);
	// Test that the error condition happened.
	if ( !ferror(fp) )
		errx(1, "could not cause a ferror condition");
	return 0;
}
