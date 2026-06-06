/* Test whether a basic getgrgid_r invocation works. */

#include <grp.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t gid = getgid();
	long reasonable = sysconf(_SC_GETGR_R_SIZE_MAX);
	size_t size = 0 < reasonable ? reasonable : 64;
	char* buffer = malloc(size);
	if ( !buffer )
		err(1, "malloc");
	struct group entry;
	struct group* grp;
	while ( (errno = getgrgid_r(gid, &entry, buffer, size, &grp)) )
	{
		if ( errno == ERANGE )
		{
			size *= 2;
			if ( !(buffer = realloc(buffer, size)) )
				err(1, "malloc");
			continue;
		}
		err(1, "getgrgid_r");
	}
	if ( !grp )
		errx(1, "group not found");
	if ( grp->gr_gid != gid )
		errx(1, "wrong gid");
	return 0;
}
