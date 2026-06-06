/* Test whether a basic strpbrk invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "abcdefg";
	if ( strpbrk(buf, "eg") != buf + 4 )
		errx(1, "strpbrk did not find 'e'");
	if ( strpbrk(buf, "x") )
		errx(1, "strpbrk found absent character");
	return 0;
}
