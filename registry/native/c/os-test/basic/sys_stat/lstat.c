/* Test whether a basic lstat invocation works. */

#include <sys/stat.h>

#include "../basic.h"

int main(void)
{
	struct stat st;
	if ( lstat("sys_stat/lstat", &st) < 0 )
		err(1, "stat");
	if ( !S_ISREG(st.st_mode) )
		errx(1, "sys_stat/lstat is not a file");
	return 0;
}
