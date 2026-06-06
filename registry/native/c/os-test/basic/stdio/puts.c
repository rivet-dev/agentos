/* Test whether a basic puts invocation works. */

#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( puts("foo") == EOF )
		err(1, "puts");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	char buf[256];
	size_t amount = fread(buf, 1, sizeof(buf) - 1, stdin);
	if ( ferror(stdin) )
		err(1, "fread");
	buf[amount] = '\0';
	const char* expected = "foo\n";
	if ( strcmp(buf, expected) != 0 )
		errx(1, "puts wrote '%s' instead of '%s'", buf, expected);
	return 0;
}
