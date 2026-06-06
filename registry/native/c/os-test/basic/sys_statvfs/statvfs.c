/* Test whether a basic statvfs invocation works. */

#include <sys/statvfs.h>

#include "../basic.h"

int main(void)
{
	struct statvfs stvfs;
	if ( statvfs(".", &stvfs) < 0 )
		err(1, "statvfs");
	return 0;
}
