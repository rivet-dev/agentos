/*[MLR]*/
/* Test whether a basic munlock invocation works. */

#include <sys/mman.h>

#include <stdint.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	long pagesize = sysconf(_SC_PAGESIZE);
	if ( pagesize < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	void* page = (void*) ((uintptr_t) &pagesize & ~((uintptr_t) (pagesize-1)));
	if ( mlock(page, pagesize) < 0 )
	{
		if ( errno == EPERM )
			return 0;
		err(1, "mlock");
	}
	if ( munlock(page, pagesize) < 0 )
		err(1, "munlock");
	return 0;
}
