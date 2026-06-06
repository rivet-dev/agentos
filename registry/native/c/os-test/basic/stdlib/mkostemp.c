/* Test whether a basic mkostemp invocation works. */

#include <fcntl.h>
#include <stdlib.h>
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
	int fd = mkostemp(template, O_CLOEXEC | O_APPEND);
	if ( fd < 0 )
		err(1, "mkostemp");
	unlink(template);
	if ( fcntl(fd, F_GETFD) != FD_CLOEXEC )
		errx(1, "fcntl(F_GETFD) != FD_CLOEXEC");
	if ( !(fcntl(fd, F_GETFL) & O_APPEND) )
		errx(1, "!(fcntl(F_GETFL) & O_APPEND)");
	return 0;
}
