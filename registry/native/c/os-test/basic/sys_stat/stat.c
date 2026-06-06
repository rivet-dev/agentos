/* Test whether a basic stat invocation works. */

#include <sys/stat.h>

#include "../basic.h"

int main(void)
{
	struct stat st;
	if ( stat("sys_stat/stat", &st) < 0 )
		err(1, "stat");
	if ( !S_ISREG(st.st_mode) )
		errx(1, "sys_stat/stat is not a file");
	return 0;
}
