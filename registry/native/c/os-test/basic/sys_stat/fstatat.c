/* Test whether a basic fstatat invocation works. */

#include <sys/stat.h>

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	int dirfd = open("..", O_RDONLY | O_DIRECTORY);
	if ( dirfd < 0 )
		err(1, "open: ..");
	struct stat st;
	if ( fstatat(dirfd, "basic/sys_stat/fstatat", &st, AT_SYMLINK_NOFOLLOW) < 0 )
		err(1, "fstatat");
	if ( !S_ISREG(st.st_mode) )
		errx(1, "basic/sys_stat/fstatat is not a file");
	return 0;
}
