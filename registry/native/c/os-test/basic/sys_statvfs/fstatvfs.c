/* Test whether a basic fstatvfs invocation works. */

#include <sys/statvfs.h>

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	int fd = open("sys_statvfs", O_RDONLY | O_DIRECTORY);
	if ( fd < 0 )
		err(1, "sys_statvfs");
	struct statvfs stvfs;
	if ( fstatvfs(fd, &stvfs) < 0 )
		err(1, "fstatvfs");
	return 0;
}
