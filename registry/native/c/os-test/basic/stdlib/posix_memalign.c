/*[ADV]*/
/* Test whether a basic posix_memalign invocation works. */

#include <stdint.h>
#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	void* ptr;
	int errnum = posix_memalign(&ptr, 256, 768);
	if ( errnum )
	{
		errno = errnum;
		err(1, "posix_memalign");
	}
	uintptr_t intptr = (uintptr_t) ptr;
	if ( intptr & 0xFF )
		errx(1, "posix_memalign did not 256-byte align");
	return 0;
}
