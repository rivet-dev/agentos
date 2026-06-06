/* Test whether a basic opendir invocation works. */

#include <dirent.h>

#include "../basic.h"

int main(void)
{
	DIR* dir = opendir(".");
	if ( !dir )
		err(1, "opendir");
	return 0;
}
