/* Test whether a basic closedir invocation works. */

#include <dirent.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	if ( closedir(dir) < 0 )
		err(1, "closedir");
	return 0;
}
