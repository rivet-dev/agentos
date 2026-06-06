/* Test whether a basic lseek invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fd = open("unistd/lseek", O_RDONLY);
	if ( fd < 0 )
		err(1, "open: unistd/lseek");
	off_t size = lseek(fd, 0, SEEK_END);
	if ( size < 0 )
		err(1, "lseek");
	if ( size == 0 )
		errx(1, "wrong size");
	return 0;
}
