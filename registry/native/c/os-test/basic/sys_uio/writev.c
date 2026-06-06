/*[XSI]*/
/* Test whether a basic writev invocation works. */

#include <sys/uio.h>

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"


int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	rewind(fp);
	char buf1[] = "foo";
	char buf2[] = "barqux";
	struct iovec iov[2] =
	{
		{ .iov_base = buf1, .iov_len = sizeof(buf1) - 1 },
		{ .iov_base = buf2, .iov_len = sizeof(buf2) - 1 },
	};
	ssize_t amount = writev(fileno(fp), iov, 2);
	if ( amount < 0 )
		err(1, "writev");
	if ( amount != 9 )
		errx(1, "writev() != 9");
	lseek(fileno(fp), 0, SEEK_SET);
	char output[16];
	if ( !fgets(output, sizeof(output), fp) )
		errx(1, "fgets");
	const char* expected = "foobarqux";
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" wanted \"%s\"", output, expected);
	return 0;
}
