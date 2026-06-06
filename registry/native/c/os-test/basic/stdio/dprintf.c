/* Test whether a basic dprintf invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	int ret = dprintf(fds[1], "hello %s %d", "world", 42);
	if ( ret < 0 )
		err(1, "dprintf");
	const char* expected = "hello world 42";
	if ( (size_t) ret != strlen(expected) )
		errx(1, "dprintf returned wrong length");
	char buffer[256];
	ssize_t amount = read(fds[0], buffer, sizeof(buffer) - 1);
	if ( amount < 0 )
		err(1, "read");
	buffer[amount] = 0;
	if ( strcmp(buffer, expected) != 0 )
		errx(1, "dprintf wrote '%s' instead of '%s'", buffer, expected);
	return 0;
}
