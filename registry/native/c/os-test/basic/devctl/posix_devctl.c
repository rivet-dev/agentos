/*[DC]*/
/* Test whether a basic posix_devctl invocation works. */

#include <devctl.h>

#include "../basic.h"

int (*foo)(int, int, void *restrict, size_t, int *restrict) = posix_devctl;

int main(void)
{
	// There is no standardized way to actually invoke posix_devctl. In my
	// opinion, that means it shouldn't be in the standard. However, since there
	// is no way to actually test this function, the test will pass if the
	// function is declared correctly.
	return 0;
}
