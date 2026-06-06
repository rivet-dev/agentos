/* Test whether a basic truncate invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
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
	if ( truncate(template, 42) < 0 )
	{
		warn("truncate");
		unlink(template);
		exit(1);
	}
	if ( lseek(fd, 0, SEEK_END) != 42 )
	{
		warnx("lseek did not return 42");
		unlink(template);
		exit(1);
	}
	unlink(template);
	return 0;
}
