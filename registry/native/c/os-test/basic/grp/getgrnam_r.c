/* Test whether a basic getgrnam_r invocation works. */

#include <grp.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t gid = getgid();
	struct group* grp = getgrgid(gid);
	if ( !grp )
		err(1, "getgrgid");
	char* group = strdup(grp->gr_name);
	if ( !group )
		errx(1, "malloc");
	long reasonable = sysconf(_SC_GETGR_R_SIZE_MAX);
	size_t size = 0 < reasonable ? reasonable : 64;
	char* buffer = malloc(size);
	if ( !buffer )
		err(1, "malloc");
	struct group entry;
	while ( (errno = getgrnam_r(group, &entry, buffer, size, &grp)) )
	{
		if ( errno == ERANGE )
		{
			size *= 2;
			if ( !(buffer = realloc(buffer, size)) )
				err(1, "malloc");
			continue;
		}
		err(1, "getgrnam_r");
	}
	if ( !grp )
		errx(1, "group not found");
	if ( strcmp(grp->gr_name, group) != 0 )
		errx(1, "wrong name");
	return 0;
}
