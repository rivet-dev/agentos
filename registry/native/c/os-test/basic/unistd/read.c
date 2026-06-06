/* Test whether a basic read invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fd = open("unistd/read", O_RDONLY);
	if ( fd < 0 )
		err(1, "open: unistd/read");
	char c;
	ssize_t amount = read(fd, &c, 1);
	if ( amount < 0 )
		err(1, "read");
	else if ( amount == 0 )
		errx(1, "read: EOF");
	else if ( amount != 1 )
		errx(1, "read() != -1");
	return 0;
}
