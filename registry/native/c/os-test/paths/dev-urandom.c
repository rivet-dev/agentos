/* Test whether /dev/urandom exists and returns random numbers. */

#include "suite.h"

int main(void)
{
	const char* path = "/dev/urandom";
	int fd = open(path, O_RDONLY);
	if ( fd < 0 )
		err(1, "%s", path);
	char c;
	ssize_t amount = read(fd, &c, 1);
	if ( amount < 0 )
		err(1, "read");
	if ( !amount )
		errx(1, "%s: unexpected EOF", path);
	puts("Yes");
	return 0;
}
