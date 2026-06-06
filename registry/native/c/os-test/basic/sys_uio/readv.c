/*[XSI]*/
/* Test whether a basic readv invocation works. */

#include <sys/uio.h>

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputs("foobar", fp) == EOF || ferror(fp) )
		err(1, "fputs");
	rewind(fp);
	char buf1[3] = "";
	char buf2[8] = "";
	struct iovec iov[2] =
	{
		{ .iov_base = buf1, .iov_len = sizeof(buf1) },
		{ .iov_base = buf2, .iov_len = sizeof(buf2) },
	};
	ssize_t amount = readv(fileno(fp), iov, 2);
	if ( amount < 0 )
		err(1, "readv");
	if ( amount != 6 )
		errx(1, "readv() != 6");
	if ( memcmp(buf1, "foo", 3) != 0 )
		errx(1, "buf1 was not foo");
	if ( memcmp(buf2, "bar\0\0\0\0\0", 8) != 0 )
		errx(1, "buf2 was not bar");
	return 0;
}
