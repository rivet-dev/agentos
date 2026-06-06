/* Test whether a basic munmap invocation works. */

#include <sys/mman.h>

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	long pagesize = sysconf(_SC_PAGESIZE);
	if ( pagesize < 0 )
		err(1, "sysconf _SC_PAGESIZE");
	void* ptr = mmap(NULL, pagesize, PROT_READ | PROT_WRITE,
	                 MAP_ANONYMOUS | MAP_PRIVATE, -1, 0);
	if ( ptr == MAP_FAILED )
		err(1, "mmap");
	if ( munmap(ptr, pagesize) < 0 )
		err(1, "munmap");
	return 0;
}
