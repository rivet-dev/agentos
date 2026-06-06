/* Test whether a basic getgroups invocation works. */

#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int ngroups = getgroups(0, NULL);
	if ( ngroups < 0 )
		err(1, "getgroups");
	if ( !ngroups )
		ngroups = 1;
	gid_t* groups = calloc(ngroups, sizeof(gid_t));
	if ( !groups )
		err(1, "malloc");
	if ( getgroups(ngroups, groups) < 0 )
		err(1, "getgroups");
	return 0;
}
