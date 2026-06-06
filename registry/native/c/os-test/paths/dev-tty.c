/* Test whether the /dev/tty file exists. */

#include "suite.h"

int main(void)
{
	// POSIX requires /dev/tty to exist.
	const char* path = "/dev/tty";
	// The test might not be run from within a session with a tty.
	if ( access(path, F_OK) < 0 && errno != ENXIO && errno != ENOTTY )
		err(1, "%s", path);
	puts("Yes");
	return 0;
}
