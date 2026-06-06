/*[ADV]*/
/* Test whether a basic posix_madvise invocation works. */

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
	if ( posix_madvise(page, pagesize, POSIX_MADV_SEQUENTIAL) < 0 )
		err(1, "posix_madvise");
	return 0;
}
