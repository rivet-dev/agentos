/* Test whether a basic getgrgid invocation works. */

#include <grp.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	gid_t gid = getgid();
	struct group* grp = getgrgid(gid);
	if ( !grp )
		err(1, "getgrgid");
	if ( grp->gr_gid != gid )
		errx(1, "wrong gid");
	return 0;
}
