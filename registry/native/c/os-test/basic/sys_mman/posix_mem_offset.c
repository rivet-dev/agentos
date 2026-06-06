/*[TYM]*/
/* Test whether a basic posix_mem_offset invocation works. */

#include <sys/mman.h>

#include "../basic.h"

int main(void)
{
	// Unfortunately it's impossible to actually test this function:
	//
	//   "If the memory object specified by fildes is not a typed memory object,
	//    then the behavior of this function is implementation-defined."
	//
	//   "Unlike shared memory objects, there is no way within POSIX.1-2024
	//    for a program to create a typed memory object."
	//
	// Just assume it works if it's declared.
	exit(0);

	posix_mem_offset(NULL, 0, NULL, NULL, NULL);
	return 0;
}
