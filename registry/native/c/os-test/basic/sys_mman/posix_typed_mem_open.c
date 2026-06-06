/*[TYM]*/
/* Test whether a basic posix_typed_mem_open invocation works. */

#include <sys/mman.h>

#include "../basic.h"

int main(void)
{
	// Unfortunately it's impossible to actually test this function:
	//
	//   "Unlike shared memory objects, there is no way within POSIX.1-2024
	//    for a program to create a typed memory object."
	//
	// Just assume it works if it's declared.
	exit(0);

	posix_typed_mem_open("", 0, 0);
	return 0;
}
