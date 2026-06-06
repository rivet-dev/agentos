/* Test whether a basic fstat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	int fd = open("sys_stat/fstat", O_RDONLY);
	if ( fd < 0 )
		err(1, "open: sys_stat/fstat");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		err(1, "stat");
	if ( !S_ISREG(st.st_mode) )
		errx(1, "sys_stat/fstat is not a file");
	return 0;
}
