/*[XSI]*/
/* Test whether a basic getdate invocation works. */

#include <time.h>
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
	FILE* fp = fdopen(fd, "w");
	if ( fprintf(fp, "%%Y-%%m-%%d %%H:%%M:%%S\n") < 0 )
		err(1, "fprintf: os-test.XXXXXX");
	if ( ferror(fp) || fflush(fp) == EOF )
		err(1, "fflush: os-test.XXXXXX");
	fclose(fp);
	if ( setenv("DATEMSK", template, 1) < 0 )
		err(1, "setenv");
	struct tm* tm = getdate("2000-01-01 02:03:04");
	if ( !tm )
		errx(1, "getdate failed: %i", getdate_err);
	if ( tm->tm_year != 100 )
		errx(1, "getdate gave year %d not %d", 1900 + tm->tm_year, 1900 + 100);
	if ( tm->tm_mon != 0 )
		errx(1, "getdate gave month %d not %d", tm->tm_mon + 1, 0 + 1);
	if ( tm->tm_mday != 1 )
		errx(1, "getdate gave day %d not %d", tm->tm_mday, 1);
	if ( tm->tm_hour != 2 )
		errx(1, "getdate gave hour %d not %d", tm->tm_hour, 2);
	if ( tm->tm_min != 3 )
		errx(1, "getdate gave min %d not %d", tm->tm_min, 3);
	if ( tm->tm_sec != 4 )
		errx(1, "getdate gave sec %d not %d", tm->tm_sec, 4);
	return 0;
}
