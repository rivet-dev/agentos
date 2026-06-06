/* Test whether a basic getgrnam invocation works. */

#include <grp.h>
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
	grp = getgrnam(group);
	if ( !grp )
		err(1, "getgrnam");
	if ( grp->gr_gid != gid )
		errx(1, "wrong gid");
	if ( strcmp(grp->gr_name, group) != 0 )
		errx(1, "wrong name");
	return 0;
}
