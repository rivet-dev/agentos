/* Test whether a basic strcspn invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "abcdefg";
	if ( strcspn(buf, "eg") != 4 )
		errx(1, "strcspn did not find 'e'");
	if ( strcspn(buf, "x") != 7 )
		errx(1, "strcspn found absent character");
	return 0;
}
