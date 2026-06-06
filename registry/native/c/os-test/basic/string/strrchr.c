/* Test whether a basic strrchr invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "foo/bar/qux";
	if ( strrchr(buf, '/') != buf + 7 )
		errx(1, "strrchr did not return last '/'");
	if ( strrchr(buf, 'X') )
		errx(1, "strrchr found absent character");
	return 0;
}
